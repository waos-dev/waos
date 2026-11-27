//! The services that keep WaOS's apps, put together once, at WaOS's start or a test's: the apps
//! running (`running_apps`), the package manager, which installs, removes and loads them
//! (`package_manager`), and the one way to start an installed app (`app_starter`), with the engine
//! they run on, what each runs with (`environments`), where its views go (`program_places`) and
//! what it writes (`launch_outputs`). Whoever shows or drives apps (the library, the dev server,
//! the programs' apps on macOS) is handed these, as Android hands an app's process its system
//! services.
//!
//! The invariant: each service is made once, here, from the others it needs, so no service is
//! handed another after it is made; the running apps alone are made before the engine, since what
//! an inspector sees of the engine's apps reads them.

use std::sync::Arc;

use waos::WaOS;
use waos_store::Store;

use super::app_starter::AppStarter;
use super::environments::ProgramEnvironments;
use super::launch_outputs::LaunchOutputs;
use super::package_manager::PackageManager;
use super::program_places::ProgramPlaces;
use super::running_apps::RunningApps;

/// The services that keep WaOS's apps.
#[derive(Clone)]
pub(crate) struct AppServices {
    pub(crate) running: Arc<RunningApps>,
    pub(crate) packages: Arc<PackageManager>,
    pub(crate) starter: Arc<AppStarter>,
    pub(crate) engine: Arc<WaOS>,
    pub(crate) environments: Arc<ProgramEnvironments>,
    pub(crate) places: ProgramPlaces,
    /// What each app writes, which the dev server's attaches read; a phone has no dev server yet.
    #[cfg(desktop)]
    pub(crate) outputs: Arc<LaunchOutputs>,
}

impl AppServices {
    /// The services over the apps installed in `store`, running among `running` on `engine`,
    /// each with what `environments` gives it, its views going where `places` says.
    pub(crate) fn new(
        running: Arc<RunningApps>,
        engine: Arc<WaOS>,
        store: Store,
        environments: Arc<ProgramEnvironments>,
        places: ProgramPlaces,
    ) -> AppServices {
        let packages = Arc::new(PackageManager::new(
            store,
            Arc::clone(&engine),
            Arc::clone(&running),
        ));
        let outputs = Arc::new(LaunchOutputs::default());
        let starter = Arc::new(AppStarter::new(
            Arc::clone(&running),
            Arc::clone(&packages),
            Arc::clone(&engine),
            Arc::clone(&environments),
            places.clone(),
            Arc::clone(&outputs),
        ));
        AppServices {
            running,
            packages,
            starter,
            engine,
            environments,
            places,
            #[cfg(desktop)]
            outputs,
        }
    }
}
