//! The store: the sources the user added and their downloads, what the user lets each app use,
//! each app's own folders and its activity, kept in a directory the app gives it.
//!
//! The invariant a later editor will break: every change to the sources, the settings and the
//! apps' own folders happens under one lock, so two edits never interleave; and a source's listing
//! of a development id, which no package takes, is refused before its download.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use url::Url;
use waos_capabilities::{ChangedFuture, ProgramFolders, ProgramGrants};
use waos_package::{Index, ListedProgram, ProgramId};

use crate::fetch::Download;
use crate::grants::{Settings, StoreGrants};
use crate::{
    GlobalSettings, ProgramSettings, Source, SourceError, SourceProxy, StoreError, fetch, programs,
    sources_file,
};

/// The folder of installed programs, in the store's directory.
const PROGRAMS: &str = "programs";

/// The file of sources, in the store's directory.
const SOURCES: &str = "sources.toml";

/// The sources the user added, what the user lets each app use, and each app's own folders and
/// activity. One store opens a directory; clones share it.
#[derive(Clone)]
pub struct Store {
    directory: PathBuf,
    /// Held while the directory changes, so changes happen one at a time.
    changes: Arc<Mutex<()>>,
    http: reqwest::Client,
    /// What the user lets programs use, held in memory over its files.
    settings: Arc<Settings>,
}

impl Store {
    /// The store in `directory`, made when it is not there yet, reading sources through `proxy`.
    pub fn open(directory: &Path, proxy: SourceProxy) -> Result<Store, StoreError> {
        let store = Store {
            directory: directory.to_path_buf(),
            changes: Arc::default(),
            http: fetch::http_client(proxy)?,
            settings: Arc::new(Settings::new(directory)),
        };
        let programs = store.programs_folder();
        fs::create_dir_all(&programs).map_err(StoreError::io_at(&programs))?;
        Ok(store)
    }

    /// Starts downloading `program` as `source` lists it: its package's bytes as they arrive,
    /// for the package manager to take in, ending in an error unless their SHA-256 is the one
    /// listed. A listing of a development install's id, which no package may take, is refused
    /// before anything is downloaded.
    pub async fn download(
        &self,
        source: &Source,
        program: &ListedProgram,
    ) -> Result<Download, SourceError> {
        if program.id.is_development() {
            return Err(SourceError::DevelopmentId(program.id.clone()));
        }
        let url = source.package_url(&program.package)?;
        fetch::download(&self.http, &url, &program.sha256).await
    }

    /// The app `id` was removed, its folder with it: what the store holds of it in memory, and
    /// its activity, go.
    pub fn forget(&self, id: &ProgramId) {
        let _changing = self.lock_changes();
        self.settings.forget(id);
        self.forget_days(id.as_str());
    }

    /// The program `id`'s own folders, which it sees at `/data` and `/cache`, made at its first
    /// run. Installed or run in development, the store keeps them in its folder until the app is
    /// removed.
    pub fn folders(&self, id: &ProgramId) -> Result<ProgramFolders, StoreError> {
        let _changing = self.lock_changes();
        programs::own_folders(&self.program_folder(id))
    }

    /// The sources the user added, in the order added; none when they cannot be read, which is
    /// said to whoever subscribes.
    pub fn sources(&self) -> Vec<Source> {
        sources_file::read(&self.sources_path()).unwrap_or_else(|error| {
            tracing::warn!("the sources cannot be read: {error}");
            Vec::new()
        })
    }

    /// Makes the list of sources `first` where none was ever made, as WaOS comes with sources of
    /// its own. A list made already is kept as it is, emptied or not, so a source the user removed
    /// never comes back.
    pub fn start_sources_with(&self, first: &[Source]) -> Result<(), StoreError> {
        let _changing = self.lock_changes();
        let path = self.sources_path();
        if sources_file::is_made(&path)? {
            return Ok(());
        }
        sources_file::write(&path, first)
    }

    /// Adds `source` after the others; a source added already stays where it is.
    pub fn add_source(&self, source: Source) -> Result<(), StoreError> {
        let _changing = self.lock_changes();
        let path = self.sources_path();
        let mut sources = sources_file::read(&path)?;
        if !sources.contains(&source) {
            sources.push(source);
            sources_file::write(&path, &sources)?;
        }
        Ok(())
    }

    /// Removes `source` from the sources; one not among them is refused.
    pub fn remove_source(&self, source: &Source) -> Result<(), StoreError> {
        let _changing = self.lock_changes();
        let path = self.sources_path();
        let mut sources = sources_file::read(&path)?;
        let count = sources.len();
        sources.retain(|added| added != source);
        if sources.len() == count {
            return Err(StoreError::NotAdded(source.clone()));
        }
        sources_file::write(&path, &sources)
    }

    /// The settings every program follows until the user sets its own.
    pub fn global_settings(&self) -> Result<GlobalSettings, StoreError> {
        self.settings.global()
    }

    /// Changes the global settings as `change` does; every program without its own hears it.
    pub fn change_global_settings(
        &self,
        change: impl FnOnce(&mut GlobalSettings),
    ) -> Result<(), StoreError> {
        let _changing = self.lock_changes();
        self.settings.change_global(change)?;
        self.forget_unrecorded_days();
        Ok(())
    }

    /// The program `id`'s own settings, each the global one until the user sets it: a program
    /// installed, or run in development, whose folder the store keeps.
    pub fn program_settings(&self, id: &ProgramId) -> Result<ProgramSettings, StoreError> {
        let folder = self.kept_program_folder(id)?;
        self.settings.program(id, &folder)
    }

    /// Changes the program `id`'s own settings as `change` does, installed or run in development,
    /// or set before its first run, which makes its folder; the program hears it.
    pub fn change_program_settings(
        &self,
        id: &ProgramId,
        change: impl FnOnce(&mut ProgramSettings),
    ) -> Result<(), StoreError> {
        let _changing = self.lock_changes();
        let folder = self.keep_program(id)?;
        self.settings.change_program(id, &folder, change)?;
        self.forget_unrecorded_days();
        Ok(())
    }

    /// What the user lets the program `id` use, installed or run in development, read live from
    /// its settings, which keep its answers.
    pub fn grants(&self, id: &ProgramId) -> Arc<dyn ProgramGrants> {
        Arc::new(StoreGrants {
            store: self.clone(),
            id: id.clone(),
        })
    }

    /// Resolves at the next change of any settings, the global ones or a program's own.
    pub fn settings_changed(&self) -> ChangedFuture {
        self.settings.changed()
    }

    /// The global settings and the program `id`'s own.
    pub(crate) fn settings_of(
        &self,
        id: &ProgramId,
    ) -> Result<(GlobalSettings, ProgramSettings), StoreError> {
        Ok((self.global_settings()?, self.program_settings(id)?))
    }

    /// The index `source` serves: the programs it offers, each at a version, with its package.
    /// An address that answers but has no index is told apart from one that could not be read:
    /// it is no source, as a website that lists no apps.
    pub async fn read_source(&self, source: &Source) -> Result<Index, SourceError> {
        let read = fetch::read_index(&self.http, &source.index_url()).await;
        read.map_err(|error| match error {
            SourceError::Status { url, status } if status == StatusCode::NOT_FOUND => {
                SourceError::ListsNothing { url }
            }
            other => other,
        })
    }

    /// The picture `source`'s index lists as `picture`, an icon or a screenshot, as its file
    /// holds it: read from the source's own server the first time and kept in the store's
    /// `pictures/`, by its address, from then on, so a store shows it at once and offline. A
    /// picture's address names its app's version, so a new version's is another file.
    pub async fn picture(&self, source: &Source, picture: &str) -> Result<Vec<u8>, SourceError> {
        let url = source.picture_url(picture)?;
        let kept = self.kept_picture(&url);
        if let Ok(bytes) = fs::read(&kept) {
            return Ok(bytes);
        }
        let bytes = fetch::read_picture(&self.http, &url).await?;
        // A picture that cannot be kept is still shown; it is read again next time.
        let keeping = fs::create_dir_all(self.pictures())
            .and_then(|()| crate::files::write_whole(&kept, &bytes));
        if let Err(error) = keeping {
            tracing::warn!(path = %kept.display(), %error, "a source's picture is not kept");
        }
        Ok(bytes)
    }

    /// Removes the pictures kept that are none of `shown`, the addresses the sources read now
    /// list: an app's older version's, and a removed source's.
    pub fn keep_pictures(&self, shown: &[Url]) {
        let kept: HashSet<PathBuf> = shown.iter().map(|url| self.kept_picture(url)).collect();
        let Ok(files) = fs::read_dir(self.pictures()) else {
            return;
        };
        for file in files.flatten() {
            if !kept.contains(&file.path()) {
                let _ = fs::remove_file(file.path());
            }
        }
    }

    /// Where the picture at `url` is kept: a file named by its address's SHA-256.
    fn kept_picture(&self, url: &Url) -> PathBuf {
        let name = hex::encode(Sha256::digest(url.as_str().as_bytes()));
        self.pictures().join(name)
    }

    /// The folder of the sources' pictures.
    fn pictures(&self) -> PathBuf {
        self.directory.join("pictures")
    }

    pub(crate) fn lock_changes(&self) -> MutexGuard<'_, ()> {
        // The lock guards no value, so a change that panicked leaves nothing half-changed in it.
        self.changes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn programs_folder(&self) -> PathBuf {
        self.directory.join(PROGRAMS)
    }

    /// The store's directory.
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    /// Whether the store has a folder of the program named `program`: installed, or run in
    /// development.
    pub(crate) fn has_folder_of(&self, program: &str) -> bool {
        self.programs_folder().join(program).is_dir()
    }

    fn program_folder(&self, id: &ProgramId) -> PathBuf {
        self.programs_folder().join(id.as_str())
    }

    /// The folder of the program `id`, which the store keeps while it is installed or once it
    /// ran in development.
    fn kept_program_folder(&self, id: &ProgramId) -> Result<PathBuf, StoreError> {
        let folder = self.program_folder(id);
        if folder.is_dir() {
            Ok(folder)
        } else {
            Err(StoreError::NotInstalled(id.clone()))
        }
    }

    /// The folder of the program `id`, made so a setting is kept there before its install or its
    /// first run; the caller holds the store's lock.
    fn keep_program(&self, id: &ProgramId) -> Result<PathBuf, StoreError> {
        let folder = self.program_folder(id);
        fs::create_dir_all(&folder).map_err(StoreError::io_at(&folder))?;
        Ok(folder)
    }

    fn sources_path(&self) -> PathBuf {
        self.directory.join(SOURCES)
    }
}
