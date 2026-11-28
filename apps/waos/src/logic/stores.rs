//! Where each of WaOS's domains keeps what it owns ([`Stores`]), which the one startup
//! (`startup`) opens the domains over: WaOS's files in its store's directory, or memory alone for
//! a test, so a test and the product assemble the domains the same way.
//!
//! The invariant: a domain reaches what it keeps only through its store here, so a WaOS over
//! [`Stores::in_memory`] writes none of it to disk.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use waos_services::activity::{ActivityFiles, ActivityStore};
use waos_services::packages::{InstalledAppsFiles, InstalledAppsStore};
use waos_services::permissions::{SettingsFiles, SettingsStore};
use waos_services::sources::{SourcesFile, SourcesStore};

/// Where each domain keeps what it owns.
pub(crate) struct Stores {
    /// The package manager's installed apps.
    pub(crate) installed_apps: Arc<dyn InstalledAppsStore>,
    /// Permissions' settings: what the user lets each app use.
    pub(crate) settings: Arc<dyn SettingsStore>,
    /// The sources the user added, and the pictures read from them.
    pub(crate) sources: Arc<dyn SourcesStore>,
    /// What each app did.
    pub(crate) activity: Arc<dyn ActivityStore>,
}

impl Stores {
    /// The stores of a WaOS kept in `folder`, its store's directory.
    pub(crate) fn files(folder: &Path) -> Result<Stores> {
        let installed_apps = InstalledAppsFiles::open(folder)
            .with_context(|| format!("opening the apps installed at {}", folder.display()))?;
        Ok(Stores {
            installed_apps: Arc::new(installed_apps),
            settings: Arc::new(SettingsFiles::new(folder)),
            sources: Arc::new(SourcesFile::new(folder)),
            activity: Arc::new(ActivityFiles::new(folder)),
        })
    }
}

#[cfg(test)]
mod in_memory {
    use waos_services::activity::ActivityInMemory;
    use waos_services::packages::{AppInMemory, InstalledAppsInMemory};
    use waos_services::permissions::SettingsInMemory;
    use waos_services::sources::SourcesInMemory;

    use super::*;

    impl Stores {
        /// Stores that keep everything in memory, with `apps` installed already.
        pub(crate) fn in_memory(apps: Vec<AppInMemory>) -> Stores {
            Stores {
                installed_apps: Arc::new(InstalledAppsInMemory::with(apps)),
                settings: Arc::new(SettingsInMemory::default()),
                sources: Arc::new(SourcesInMemory::default()),
                activity: Arc::new(ActivityInMemory::default()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::Stores;
    use crate::logic::app_starter::{Opening, StartRequest};
    use crate::logic::startup::Setup;
    use crate::logic::startup::tests::start_asking_nobody;
    use crate::logic::test_engine::{self, NoWindows};
    use crate::logic::test_installs;

    const FOREVER: &str = "dev.example.forever";

    /// Every file under `folder`, at any depth; none of the folders.
    fn files_under(folder: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut files = Vec::new();
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                files.extend(files_under(&entry.path()));
            } else {
                files.push(entry.path());
            }
        }
        files
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_app_kept_in_memory_starts_from_its_build_with_no_file_in_the_store() {
        let folder = tempfile::tempdir().unwrap();
        let app = test_installs::built_app(waos_test_programs::FOREVER, FOREVER, "Forever");
        let setup = Setup::for_tests(folder.path(), Stores::in_memory(vec![app]));
        let app_folders = setup.app_folders.clone();
        let started = start_asking_nobody(setup, |_before| {
            let shell = test_engine::shell(Arc::new(NoWindows));
            (shell, test_engine::no_screens())
        });

        let id = FOREVER.parse().unwrap();
        let request = StartRequest::open(FOREVER.parse().unwrap(), Opening::ByTheUser);
        started.starter.start(request).await.expect("it starts");

        assert!(started.running.running(&id).is_some(), "it runs");
        assert_eq!(files_under(&app_folders), Vec::<PathBuf>::new());
    }
}
