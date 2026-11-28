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
