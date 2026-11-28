//! The installed apps kept in memory ([`InstalledAppsInMemory`]), for tests: the package manager,
//! and everything above it, run with nothing installed on disk.
//!
//! What an install brings in, a package or a development build, is kept as the app's record and
//! icon alone: such an app is installed and listed, but has no version to run. Only an app a test
//! fills the store with may have one ([`AppInMemory::version`]).
//!
//! The invariant a later editor will break: an app's record, icon and version change together,
//! under the one lock, so no reader sees one version's record with another's icon.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use waos_package::{Package, ProgramId};

use crate::packages::error::{FolderError, InstallError, RemoveError};
use crate::packages::installed::{DevelopmentBuild, InstalledProgram, InstalledVersion, Origin};
use crate::packages::store::{InstalledAppsStore, StagedVersion};

/// One app as the in-memory store keeps it.
#[derive(Clone, Debug)]
pub struct AppInMemory {
    pub program: InstalledProgram,
    /// Its icon, a square PNG's bytes, where it has one.
    pub icon: Option<Arc<[u8]>>,
    /// Where the engine loads it from, as a version of an app the test's build made; none for an
    /// app that is only listed.
    pub version: Option<InstalledVersion>,
}

impl AppInMemory {
    /// `program`, listed alone: no icon, and no version to run.
    pub fn new(program: InstalledProgram) -> AppInMemory {
        AppInMemory {
            program,
            icon: None,
            version: None,
        }
    }
}

/// The installed apps, kept in memory alone, gone with the value.
#[derive(Default)]
pub struct InstalledAppsInMemory {
    /// Shared with the versions staged, which their commits change.
    apps: Arc<Apps>,
}

/// The apps the in-memory store keeps, by id.
type Apps = Mutex<BTreeMap<ProgramId, AppInMemory>>;

impl InstalledAppsInMemory {
    /// The store with `apps` installed.
    pub fn with(apps: impl IntoIterator<Item = AppInMemory>) -> InstalledAppsInMemory {
        let apps = apps
            .into_iter()
            .map(|app| (app.program.manifest.id().clone(), app))
            .collect();
        InstalledAppsInMemory {
            apps: Arc::new(Mutex::new(apps)),
        }
    }

    /// Puts `app` in the store, in place of the app of its id where one is there, as a test fills
    /// it with an app its build made; the package manager over the store announces no change.
    pub fn put(&self, app: AppInMemory) {
        self.locked().insert(app.program.manifest.id().clone(), app);
    }

    fn locked(&self) -> MutexGuard<'_, BTreeMap<ProgramId, AppInMemory>> {
        locked(&self.apps)
    }

    /// `program` with `icon`, staged to go in place of the installed version once committed.
    fn staged(&self, program: InstalledProgram, icon: Option<Arc<[u8]>>) -> Box<dyn StagedVersion> {
        Box::new(StagedAppInMemory {
            app: AppInMemory {
                program,
                icon,
                version: None,
            },
            apps: Arc::clone(&self.apps),
        })
    }
}

/// `apps`, for a moment: never held across an await.
fn locked(apps: &Apps) -> MutexGuard<'_, BTreeMap<ProgramId, AppInMemory>> {
    // Every change leaves the map whole, so one that panicked leaves nothing half-changed.
    apps.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An app staged in memory, its record and icon alone, put in the store's apps whole once it is
/// committed.
struct StagedAppInMemory {
    app: AppInMemory,
    apps: Arc<Apps>,
}

impl StagedVersion for StagedAppInMemory {
    fn program(&self) -> &InstalledProgram {
        &self.app.program
    }

    fn files(&self) -> Option<InstalledVersion> {
        None
    }

    fn commit(self: Box<Self>) -> Result<InstalledProgram, InstallError> {
        let program = self.app.program.clone();
        locked(&self.apps).insert(program.manifest.id().clone(), self.app);
        Ok(program)
    }
}

impl InstalledAppsStore for InstalledAppsInMemory {
    fn apps(&self) -> Vec<InstalledProgram> {
        self.locked()
            .values()
            .map(|app| app.program.clone())
            .collect()
    }

    fn app(&self, id: &ProgramId) -> Result<Option<InstalledProgram>, FolderError> {
        Ok(self.locked().get(id).map(|app| app.program.clone()))
    }

    fn icon(&self, id: &ProgramId) -> Option<Arc<[u8]>> {
        self.locked().get(id)?.icon.clone()
    }

    fn version(&self, id: &ProgramId) -> Option<InstalledVersion> {
        self.locked().get(id)?.version.clone()
    }

    /// A temporary file of the system's, since a package is opened from a file.
    fn receiving_file(&self, _app: &ProgramId) -> Result<NamedTempFile, FolderError> {
        NamedTempFile::new().map_err(FolderError::io_at(&std::env::temp_dir()))
    }

    fn stage(&self, package: &mut Package) -> Result<Box<dyn StagedVersion>, InstallError> {
        let program = InstalledProgram {
            manifest: package.manifest().clone(),
            origin: Origin::Package {
                publisher: *package.publisher(),
            },
        };
        let icon = Some(Arc::from(package.icon()));
        Ok(self.staged(program, icon))
    }

    fn stage_development(
        &self,
        build: &DevelopmentBuild,
    ) -> Result<Box<dyn StagedVersion>, InstallError> {
        let program = InstalledProgram {
            manifest: DevelopmentBuild::installed_manifest(&build.manifest),
            origin: Origin::Development {
                component_sha256: Sha256::digest(&build.component).into(),
            },
        };
        let icon = build.icon.as_deref().map(Arc::from);
        Ok(self.staged(program, icon))
    }

    fn remove(&self, id: &ProgramId) -> Result<(), RemoveError> {
        match self.locked().remove(id) {
            Some(_) => Ok(()),
            None => Err(RemoveError::NotInstalled(id.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use waos_package::Manifest;

    use super::*;

    fn notes() -> ProgramId {
        "dev.example.notes".parse().unwrap()
    }

    fn manifest(version: &str) -> Manifest {
        Manifest::new(notes(), "Notes", version.parse().unwrap()).unwrap()
    }

    /// The notes' development build at `version`, with an icon that says which it is.
    fn build(version: &str) -> DevelopmentBuild {
        DevelopmentBuild {
            manifest: manifest(version),
            component: format!("\0asm {version}").into_bytes(),
            icon: Some(format!("icon {version}").into_bytes()),
        }
    }

    fn installed_version(store: &InstalledAppsInMemory, id: &ProgramId) -> Option<String> {
        let program = store.app(id).unwrap()?;
        Some(program.manifest.version().to_string())
    }

    #[test]
    fn the_apps_it_was_filled_with_are_listed_with_their_icons_and_versions() {
        let program = InstalledProgram {
            manifest: manifest("1.0.0"),
            origin: Origin::Development {
                component_sha256: [0; 32],
            },
        };
        let version = InstalledVersion::new(
            PathBuf::from("/built/notes.wasm"),
            PathBuf::from("/built/notes-code"),
        );
        let store = InstalledAppsInMemory::with([AppInMemory {
            program: program.clone(),
            icon: Some(Arc::from(&b"a picture"[..])),
            version: Some(version.clone()),
        }]);
        assert_eq!(store.apps(), [program]);
        assert_eq!(store.icon(&notes()).as_deref(), Some(&b"a picture"[..]));
        assert_eq!(store.version(&notes()), Some(version));
    }

    #[test]
    fn a_staged_version_is_not_installed_until_it_is_committed() {
        let store = InstalledAppsInMemory::default();
        let development = notes().development();
        let staged = store.stage_development(&build("1.0.0")).unwrap();
        assert_eq!(installed_version(&store, &development), None);
        staged.commit().unwrap();
        assert_eq!(
            installed_version(&store, &development).as_deref(),
            Some("1.0.0")
        );
    }

    #[test]
    fn a_commit_replaces_the_installed_version_whole() {
        let store = InstalledAppsInMemory::default();
        let development = notes().development();
        for version in ["1.0.0", "1.1.0"] {
            let staged = store.stage_development(&build(version)).unwrap();
            staged.commit().unwrap();
        }
        assert_eq!(
            installed_version(&store, &development).as_deref(),
            Some("1.1.0")
        );
        assert_eq!(
            store.icon(&development).as_deref(),
            Some(&b"icon 1.1.0"[..])
        );
        assert_eq!(store.version(&development), None, "nothing to run");
    }

    #[test]
    fn a_removal_takes_the_app() {
        let store = InstalledAppsInMemory::default();
        let development = notes().development();
        let staged = store.stage_development(&build("1.0.0")).unwrap();
        staged.commit().unwrap();
        store.remove(&development).unwrap();
        assert_eq!(installed_version(&store, &development), None);
        assert_eq!(store.icon(&development), None);
        assert!(matches!(
            store.remove(&development),
            Err(RemoveError::NotInstalled(_))
        ));
    }
}
