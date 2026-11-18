//! The store: the programs installed, the rules a package is installed or updated by, and the
//! sources the user added, kept in a directory the app gives it.
//!
//! The invariant a later editor will break: every change to the directory happens under one lock,
//! so two installs of one program, or two edits of the sources, never interleave; and a package
//! replaces an installed program only when it is signed with the same key and is newer.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use waos_capabilities::{ChangedFuture, ProgramFolders, ProgramGrants};
use waos_package::{
    COMPONENT, ICON, Index, ListedProgram, Manifest, Package, ProgramId, PublisherKey,
};

use crate::grants::{Settings, StoreGrants};
use crate::{
    GlobalSettings, InstallError, ProgramSettings, Source, SourceError, SourceProxy, StoreError,
    fetch, programs, sources_file,
};

/// The folder of installed programs, in the store's directory.
const PROGRAMS: &str = "programs";

/// The file of sources, in the store's directory.
const SOURCES: &str = "sources.toml";

/// Where a program being removed is moved to, in the store's directory, before it is removed.
const REMOVING: &str = "removing";

/// A program as installed: its manifest, and the key every update of it must be signed with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledProgram {
    pub manifest: Manifest,
    pub publisher: PublisherKey,
}

/// A listed program's package, downloaded and checked against the SHA-256 its source lists, waiting
/// to be installed ([`Store::install_download`]); its temporary file goes when this is dropped.
pub struct Download {
    file: tempfile::NamedTempFile,
    listed: ListedProgram,
}

impl Download {
    /// The program it was downloaded as.
    pub fn listed(&self) -> &ListedProgram {
        &self.listed
    }
}

/// The programs installed in WaOS, and the sources the user added. One store opens a directory;
/// clones share it.
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
    /// Clears what installs and removals a crash interrupted left.
    pub fn open(directory: &Path, proxy: SourceProxy) -> Result<Store, StoreError> {
        let store = Store {
            directory: directory.to_path_buf(),
            changes: Arc::default(),
            http: fetch::http_client(proxy)?,
            settings: Arc::new(Settings::new(directory)),
        };
        let programs = store.programs_folder();
        fs::create_dir_all(&programs).map_err(StoreError::io_at(&programs))?;
        store.clear_unfinished();
        Ok(store)
    }

    /// Every installed program, by id. One whose files cannot be read is left out, and said so
    /// to whoever subscribes to the store's `tracing` events.
    pub fn installed(&self) -> Vec<InstalledProgram> {
        let mut installed: Vec<InstalledProgram> = self
            .program_folders()
            .iter()
            .filter_map(|folder| match programs::read_installed(folder) {
                Ok(program) => program,
                Err(error) => {
                    tracing::warn!("an installed program is left out: {error}");
                    None
                }
            })
            .collect();
        installed.sort_by(|left, right| left.manifest.id().cmp(right.manifest.id()));
        installed
    }

    /// Installs `package`: a new id, or the same id and key at a newer version, which updates
    /// it. A different key, or a version not newer, is refused, and the program installed stays
    /// as it was.
    pub fn install(&self, mut package: Package) -> Result<InstalledProgram, InstallError> {
        let _changing = self.lock_changes();
        let folder = self.program_folder(package.manifest().id());
        if let Some(installed) = programs::read_installed(&folder)? {
            check_update(&installed, &package)?;
        }
        programs::put_in_place(&folder, &mut package)?;
        Ok(InstalledProgram {
            manifest: package.manifest().clone(),
            publisher: *package.publisher(),
        })
    }

    /// Downloads `program` as `source` lists it, refuses it unless its SHA-256 is the one listed,
    /// and installs it as [`install`](Self::install) does, if it is the program and version
    /// listed: [`download`](Self::download), then [`install_download`](Self::install_download).
    pub async fn install_from(
        &self,
        source: &Source,
        program: &ListedProgram,
    ) -> Result<InstalledProgram, InstallError> {
        let download = self.download(source, program).await?;
        self.install_download(download).await
    }

    /// Downloads `program` as `source` lists it to a temporary file, and refuses it unless its
    /// SHA-256 is the one listed, before anything opens it. Nothing installed changes: a caller
    /// may end what runs of the program between this and its install.
    pub async fn download(
        &self,
        source: &Source,
        program: &ListedProgram,
    ) -> Result<Download, InstallError> {
        let url = source.package_url(&program.package)?;
        let file = fetch::download(&self.http, &url, &program.sha256).await?;
        Ok(Download {
            file,
            listed: program.clone(),
        })
    }

    /// Installs `download` as [`install`](Self::install) does, if it is the program and version
    /// listed; it is opened and installed on tokio's threads for blocking work, and its file
    /// removed after.
    pub async fn install_download(
        &self,
        download: Download,
    ) -> Result<InstalledProgram, InstallError> {
        let store = self.clone();
        let installing = tokio::task::spawn_blocking(move || {
            store.install_listed(download.file.path(), &download.listed)
        });
        installing
            .await
            .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
    }

    /// Removes the installed program `id` names, with all the store keeps of it: its settings,
    /// its own folders and its audit summary. A run of it would go on with its folders gone, so the caller ends its
    /// runs first.
    pub fn remove(&self, id: &ProgramId) -> Result<(), StoreError> {
        let _changing = self.lock_changes();
        programs::take_out(&self.program_folder(id), id, &self.removing_folder())?;
        self.settings.forget(id);
        self.forget_days(id.as_str());
        Ok(())
    }

    /// The program `id`'s own folders, which it sees at `/data` and `/cache`, made at its first
    /// run. Installed or run in development, the store keeps them in its folder until it is
    /// removed.
    pub fn folders(&self, id: &ProgramId) -> Result<ProgramFolders, StoreError> {
        let _changing = self.lock_changes();
        programs::own_folders(&self.program_folder(id))
    }

    /// The component of the program `id` names, which the app runs; none when it is not
    /// installed, or its files cannot be read, which is said to whoever subscribes.
    pub fn program(&self, id: &ProgramId) -> Option<PathBuf> {
        match programs::installed_folder(&self.program_folder(id)) {
            Ok(folder) => folder.map(|folder| folder.join(COMPONENT)),
            Err(error) => {
                tracing::warn!("{id} cannot be run: {error}");
                None
            }
        }
    }

    /// The icon of the program `id` names, a square PNG from its package; none when it is not
    /// installed, or its files cannot be read, which is said to whoever subscribes.
    pub fn icon(&self, id: &ProgramId) -> Option<PathBuf> {
        Some(self.program(id)?.with_file_name(ICON))
    }

    /// The sources the user added, in the order added; none when they cannot be read, which is
    /// said to whoever subscribes.
    pub fn sources(&self) -> Vec<Source> {
        sources_file::read(&self.sources_path()).unwrap_or_else(|error| {
            tracing::warn!("the sources cannot be read: {error}");
            Vec::new()
        })
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
    pub async fn read_source(&self, source: &Source) -> Result<Index, SourceError> {
        fetch::read_index(&self.http, &source.index_url()).await
    }

    /// Opens the package downloaded at `download` and installs it, if it is the program and
    /// version `listed`.
    fn install_listed(
        &self,
        download: &Path,
        listed: &ListedProgram,
    ) -> Result<InstalledProgram, InstallError> {
        let package = Package::open(download)?;
        check_as_listed(&package, listed)?;
        self.install(package)
    }

    /// Removes what installs and removals a crash interrupted left: in each program's folder,
    /// and the programs being removed. What cannot be removed stays, unseen, and is said to
    /// whoever subscribes.
    fn clear_unfinished(&self) {
        for folder in self.program_folders() {
            if let Err(error) = programs::clear_unfinished(&folder) {
                tracing::warn!("what an interrupted install left stays: {error}");
            }
        }
        if let Err(error) = programs::remove_if_there(&self.removing_folder()) {
            tracing::warn!("what an interrupted removal left stays: {error}");
        }
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

    fn removing_folder(&self) -> PathBuf {
        self.directory.join(REMOVING)
    }

    /// The folder of each program installed, or left by an interrupted install, and no other
    /// file the system or a person put there; none when the folder of programs cannot be read,
    /// which is said to whoever subscribes.
    fn program_folders(&self) -> Vec<PathBuf> {
        let programs = self.programs_folder();
        match fs::read_dir(&programs) {
            Ok(entries) => entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .collect(),
            Err(error) => {
                tracing::warn!("{}: {error}", programs.display());
                Vec::new()
            }
        }
    }

    fn sources_path(&self) -> PathBuf {
        self.directory.join(SOURCES)
    }
}

/// Refuses `package` as an update of `installed` unless it is signed with the same key and is
/// newer.
fn check_update(installed: &InstalledProgram, package: &Package) -> Result<(), InstallError> {
    let id = package.manifest().id();
    if *package.publisher() != installed.publisher {
        return Err(InstallError::DifferentPublisher {
            id: id.clone(),
            installed: installed.publisher,
            package: *package.publisher(),
        });
    }
    let version = package.manifest().version();
    if version.cmp_precedence(installed.manifest.version()) != Ordering::Greater {
        return Err(InstallError::NotNewer {
            id: id.clone(),
            installed: installed.manifest.version().clone(),
            package: version.clone(),
        });
    }
    Ok(())
}

/// Refuses `package` unless it is the program and version `listed`.
fn check_as_listed(package: &Package, listed: &ListedProgram) -> Result<(), InstallError> {
    let manifest = package.manifest();
    if *manifest.id() != listed.id || *manifest.version() != listed.version {
        return Err(InstallError::NotAsListed {
            listed: format!("{} {}", listed.id, listed.version),
            package: format!("{} {}", manifest.id(), manifest.version()),
        });
    }
    Ok(())
}
