//! The store: the sources the user added and their downloads, each app's own folders and its
//! activity, kept in a directory the app gives it.
//!
//! The invariant a later editor will break: every change to the sources, the apps' own folders and
//! their activity happens under one lock, so two edits never interleave; and a source's listing of
//! a development id, which no package takes, is refused before its download.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use crate::permissions::Permissions;
use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use url::Url;
use waos_capabilities::ProgramFolders;
use waos_package::{Index, ListedProgram, ProgramId};

use crate::store::fetch::Download;
use crate::store::{Source, SourceError, SourceProxy, StoreError, fetch, programs, sources_file};

/// The folder of installed programs, in the store's directory.
const PROGRAMS: &str = "programs";

/// The file of sources, in the store's directory.
const SOURCES: &str = "sources.toml";

/// The sources the user added, and each app's own folders and activity. One store opens a
/// directory; clones share it.
#[derive(Clone)]
pub struct Store {
    directory: PathBuf,
    /// Held while the directory changes, so changes happen one at a time.
    changes: Arc<Mutex<()>>,
    http: reqwest::Client,
    /// Whether each app's activity is recorded, which its settings say.
    permissions: Permissions,
}

/// A store as a thread that should end with it holds it.
pub(crate) struct WeakStore {
    directory: PathBuf,
    changes: Weak<Mutex<()>>,
    http: reqwest::Client,
    permissions: Permissions,
}

impl Store {
    /// The store in `directory`, made when it is not there yet, reading sources through `proxy`,
    /// recording each app's activity as `permissions` say: what was kept of an app whose record
    /// the user turns off goes as the setting changes, on a thread of the store's own.
    pub fn open(
        directory: &Path,
        proxy: SourceProxy,
        permissions: Permissions,
    ) -> Result<Store, StoreError> {
        let store = Store {
            directory: directory.to_path_buf(),
            changes: Arc::default(),
            http: fetch::http_client(proxy)?,
            permissions,
        };
        let programs = store.programs_folder();
        fs::create_dir_all(&programs).map_err(StoreError::io_at(&programs))?;
        store.follow_settings()?;
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

    /// The app `id` was removed, its folder with it: its activity goes.
    pub fn forget(&self, id: &ProgramId) {
        let _changing = self.lock_changes();
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
            .and_then(|()| crate::store::files::write_whole(&kept, &bytes));
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

    pub(crate) fn downgrade(&self) -> WeakStore {
        WeakStore {
            directory: self.directory.clone(),
            changes: Arc::downgrade(&self.changes),
            http: self.http.clone(),
            permissions: self.permissions.clone(),
        }
    }

    /// The store `weak` holds, while a clone of it is held elsewhere.
    pub(crate) fn upgrade(weak: &WeakStore) -> Option<Store> {
        Some(Store {
            directory: weak.directory.clone(),
            changes: weak.changes.upgrade()?,
            http: weak.http.clone(),
            permissions: weak.permissions.clone(),
        })
    }

    pub(crate) fn lock_changes(&self) -> MutexGuard<'_, ()> {
        // The lock guards no value, so a change that panicked leaves nothing half-changed in it.
        self.changes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn programs_folder(&self) -> PathBuf {
        self.directory.join(PROGRAMS)
    }

    /// Whether the store keeps the app `id`'s own folders: it is installed, or ran.
    pub fn keeps_folders_of(&self, id: &ProgramId) -> bool {
        self.has_folder_of(id.as_str())
    }

    /// What the user lets each app use, which says whether its activity is recorded.
    pub(crate) fn permissions(&self) -> &Permissions {
        &self.permissions
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

    fn sources_path(&self) -> PathBuf {
        self.directory.join(SOURCES)
    }
}
