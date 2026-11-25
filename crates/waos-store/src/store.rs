//! The store: the programs installed, the rules a package is installed or updated by, and the
//! sources the user added, kept in a directory the app gives it.
//!
//! The invariant a later editor will break: every change to the directory happens under one lock,
//! so two installs of one program, or two edits of the sources, never interleave; a package
//! replaces an installed program only when it is signed with the same key and is newer; and a
//! development build is installed only under its program's development id, which no package takes
//! (`waos-package` refuses one, and a source's listing of one is refused before its download).

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use reqwest::StatusCode;
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use url::Url;
use waos_capabilities::{ChangedFuture, ProgramFolders, ProgramGrants};
use waos_package::{
    COMPONENT, ICON, Index, ListedProgram, Manifest, Package, PackageError, ProgramId, PublisherKey,
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

/// What the name of a development install is shown with, after the program's own.
const DEVELOPMENT_LABEL: &str = " (Development)";

/// A program as installed: its manifest, and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledProgram {
    pub manifest: Manifest,
    pub origin: Origin,
}

/// Where an installed program came from: a package, whose publisher's key every update of it must
/// be signed with, or its source, built and installed under its development id by `waos install`,
/// which no package or source updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Package { publisher: PublisherKey },
    Development { component_sha256: [u8; 32] },
}

impl InstalledProgram {
    /// The key every update of it must be signed with; none for a development install.
    pub fn publisher(&self) -> Option<PublisherKey> {
        match self.origin {
            Origin::Package { publisher } => Some(publisher),
            Origin::Development { .. } => None,
        }
    }

    /// Whether it is a development install, made from its source by `waos install`.
    pub fn is_development(&self) -> bool {
        matches!(self.origin, Origin::Development { .. })
    }

    /// The name its user sees: its manifest's, and a development install's followed by
    /// " (Development)", so it reads apart from the program published under the plain id.
    pub fn name(&self) -> String {
        let name = self.manifest.name();
        match self.origin {
            Origin::Package { .. } => name.to_owned(),
            Origin::Development { .. } => format!("{name}{DEVELOPMENT_LABEL}"),
        }
    }
}

/// A program built from its source, as `waos install` sends it, to install as its development
/// install: its manifest, under the program's own id, its component and its icon, where it has
/// one.
#[derive(Clone, Copy, Debug)]
pub struct DevelopmentBuild<'a> {
    pub manifest: &'a Manifest,
    pub component: &'a [u8],
    pub icon: Option<&'a [u8]>,
}

impl DevelopmentBuild<'_> {
    /// The manifest it is installed with: its own, under the program's development id.
    fn installed_manifest(&self) -> Manifest {
        let manifest = self.manifest;
        Manifest::new(
            manifest.id().development(),
            manifest.name(),
            manifest.version().clone(),
        )
        .expect("a manifest's name is one already")
        .with_kind(manifest.kind())
        .with_category(manifest.category())
        .with_file_associations(manifest.file_associations().to_vec())
        .expect("a manifest's file associations are checked already")
    }
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
    /// Counts the changes of what is installed, for whoever shows it.
    installs: Arc<watch::Sender<u64>>,
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
            installs: Arc::new(watch::Sender::new(0)),
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

    /// The installed program `id` names; none when it is not installed, or its files cannot be
    /// read, which is said to whoever subscribes.
    pub fn installed_program(&self, id: &ProgramId) -> Option<InstalledProgram> {
        programs::read_installed(&self.program_folder(id)).unwrap_or_else(|error| {
            tracing::warn!("{id} cannot be read: {error}");
            None
        })
    }

    /// Changes each time what is installed changes: an install, an update or a removal, from here
    /// or from another clone of the store.
    pub fn installed_changes(&self) -> watch::Receiver<u64> {
        self.installs.subscribe()
    }

    /// Installs `package`: a new id, or the same id and key at a newer version, which updates
    /// it. A different key, a version not newer, or an id a development install holds, is
    /// refused, and the program installed stays as it was.
    pub fn install(&self, mut package: Package) -> Result<InstalledProgram, InstallError> {
        let _changing = self.lock_changes();
        let folder = self.program_folder(package.manifest().id());
        if let Some(installed) = programs::read_installed(&folder)? {
            check_update(&installed, &package)?;
        }
        let version = package.manifest().version().clone();
        programs::put_in_place(&folder, &version, |unpacked| Ok(package.unpack(unpacked)?))?;
        self.installed_changed();
        Ok(InstalledProgram {
            manifest: package.manifest().clone(),
            origin: Origin::Package {
                publisher: *package.publisher(),
            },
        })
    }

    /// Installs `build` as its program's development install, under the development id
    /// (`dev.example.notes.development`), a program of its own beside one installed under the
    /// plain id: new, or in place of the development build installed before, whatever its
    /// version, keeping its settings and its own folders. No package holds a development id,
    /// which `waos-package` refuses.
    pub fn install_development(
        &self,
        build: &DevelopmentBuild<'_>,
    ) -> Result<InstalledProgram, InstallError> {
        let _changing = self.lock_changes();
        let manifest = build.installed_manifest();
        let folder = self.program_folder(manifest.id());
        programs::put_in_place(&folder, manifest.version(), |unpacked| {
            Ok(programs::unpack_development(
                unpacked,
                &manifest,
                build.component,
                build.icon,
            )?)
        })?;
        self.installed_changed();
        Ok(InstalledProgram {
            manifest,
            origin: Origin::Development {
                component_sha256: programs::component_sha256(build.component),
            },
        })
    }

    /// Whether the program `manifest` names, under its own id, is installed as its development
    /// install already as it is: that manifest, its component of SHA-256 `component_sha256`, and
    /// `icon`, so installing it again would change nothing. Its component is not read.
    pub fn holds_development(
        &self,
        manifest: &Manifest,
        component_sha256: &[u8; 32],
        icon: Option<&[u8]>,
    ) -> bool {
        let build = DevelopmentBuild {
            manifest,
            component: &[],
            icon,
        };
        let folder = self.program_folder(&manifest.id().development());
        let Ok(Some(installed)) = programs::read_installed(&folder) else {
            return false;
        };
        let same_component = installed.origin
            == Origin::Development {
                component_sha256: *component_sha256,
            };
        let installed_icon = programs::installed_folder(&folder)
            .ok()
            .flatten()
            .and_then(|unpacked| programs::read_icon(&unpacked).ok());
        installed.manifest == build.installed_manifest()
            && same_component
            && installed_icon == Some(icon.map(<[u8]>::to_vec))
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
    /// SHA-256 is the one listed, before anything opens it; a listing of a development install's
    /// id, which no package may take, is refused before anything is downloaded. Nothing installed changes: a caller
    /// may end what runs of the program between this and its install.
    pub async fn download(
        &self,
        source: &Source,
        program: &ListedProgram,
    ) -> Result<Download, InstallError> {
        if program.id.is_development() {
            return Err(PackageError::DevelopmentId(program.id.clone()).into());
        }
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
        self.installed_changed();
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

    /// The manifest of the program `id` names; none when it is not installed, or its files cannot
    /// be read, which is said to whoever subscribes.
    pub fn manifest(&self, id: &ProgramId) -> Option<Manifest> {
        match programs::read_installed(&self.program_folder(id)) {
            Ok(installed) => installed.map(|installed| installed.manifest),
            Err(error) => {
                tracing::warn!("{id}'s manifest cannot be read: {error}");
                None
            }
        }
    }

    /// The icon of the program `id` names, a square PNG from its package or its development
    /// build; none when it is not installed, a development build had none, or its files cannot
    /// be read, which is said to whoever subscribes.
    pub fn icon(&self, id: &ProgramId) -> Option<PathBuf> {
        let icon = self.program(id)?.with_file_name(ICON);
        icon.is_file().then_some(icon)
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

    /// Tells whoever follows what is installed that it changed.
    fn installed_changed(&self) {
        self.installs
            .send_modify(|changes| *changes = changes.wrapping_add(1));
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
/// newer; a development install, which no package takes the id of, is no package's to update.
fn check_update(installed: &InstalledProgram, package: &Package) -> Result<(), InstallError> {
    let id = package.manifest().id();
    let Some(publisher) = installed.publisher() else {
        return Err(PackageError::DevelopmentId(id.clone()).into());
    };
    if *package.publisher() != publisher {
        return Err(InstallError::DifferentPublisher {
            id: id.clone(),
            installed: publisher,
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
