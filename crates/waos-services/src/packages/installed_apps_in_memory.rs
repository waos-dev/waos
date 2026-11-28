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
use crate::packages::store::{InstalledAppsStore, StagedContent, StagedVersion};

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
    apps: Mutex<BTreeMap<ProgramId, AppInMemory>>,
}

impl InstalledAppsInMemory {
    /// The store with `apps` installed.
    pub fn with(apps: impl IntoIterator<Item = AppInMemory>) -> InstalledAppsInMemory {
        let apps = apps
            .into_iter()
            .map(|app| (app.program.manifest.id().clone(), app))
            .collect();
        InstalledAppsInMemory {
            apps: Mutex::new(apps),
        }
    }

    /// The apps, for a moment: never held across an await.
    fn locked(&self) -> MutexGuard<'_, BTreeMap<ProgramId, AppInMemory>> {
        // Every change leaves the map whole, so one that panicked leaves nothing half-changed.
        self.apps.lock().unwrap_or_else(PoisonError::into_inner)
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

    fn stage(&self, package: &mut Package) -> Result<StagedVersion, InstallError> {
        let program = InstalledProgram {
            manifest: package.manifest().clone(),
            origin: Origin::Package {
                publisher: *package.publisher(),
            },
        };
        let icon = Some(Arc::from(package.icon()));
        Ok(StagedVersion::new(program, StagedContent::Icon(icon)))
    }

    fn stage_development(&self, build: &DevelopmentBuild) -> Result<StagedVersion, InstallError> {
        let program = InstalledProgram {
            manifest: DevelopmentBuild::installed_manifest(&build.manifest),
            origin: Origin::Development {
                component_sha256: Sha256::digest(&build.component).into(),
            },
        };
        let icon = build.icon.as_deref().map(Arc::from);
        Ok(StagedVersion::new(program, StagedContent::Icon(icon)))
    }

    fn commit(&self, staged: StagedVersion) -> Result<InstalledProgram, InstallError> {
        let (program, StagedContent::Icon(icon)) = staged.into_parts() else {
            panic!("a version is committed to the store that staged it");
        };
        let app = AppInMemory {
            program: program.clone(),
            icon,
            version: None,
        };
        self.locked().insert(program.manifest.id().clone(), app);
        Ok(program)
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
        let version = InstalledVersion::new(PathBuf::from("/built/notes"));
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
        store.commit(staged).unwrap();
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
            store.commit(staged).unwrap();
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
        store.commit(staged).unwrap();
        store.remove(&development).unwrap();
        assert_eq!(installed_version(&store, &development), None);
        assert_eq!(store.icon(&development), None);
        assert!(matches!(
            store.remove(&development),
            Err(RemoveError::NotInstalled(_))
        ));
    }
}
