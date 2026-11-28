//! The services that keep WaOS's apps, put together once, at WaOS's start or a test's: the apps
//! running (`running_apps`), the package manager (`waos_services::packages`), handed the engine's
//! compiler and the running apps, Permissions (`waos_services::permissions`), following the package
//! manager's record, the store, and the one way to start an installed app (`app_starter`), with
//! the engine they run on, what each runs with (`environments`), where its views go
//! (`program_places`) and what it writes (`launch_outputs`). Whoever shows or drives apps (the
//! library, the dev server, the programs' apps on macOS) is handed these, as Android hands an
//! app's process its system services.
//!
//! The invariant: each service is made once, from the others it needs, so no service is handed
//! another after it is made; the running apps and the package manager are made before the engine,
//! since what an inspector and the shell's parts see of the apps reads them, and the compiler the
//! package manager is handed is made before the engine for that reason.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use waos::{Compiler, WaOS};
use waos_services::packages::{CodeCompiler, InstalledApps, PackageManager, StopApp};
use waos_services::permissions::{Permissions, Prompts, SettingsFiles};
use waos_services::store::{SourceProxy, Store};

use super::app_starter::AppStarter;
use super::environments::ProgramEnvironments;
use super::launch_outputs::LaunchOutputs;
use super::program_places::ProgramPlaces;
use super::running_apps::RunningApps;

/// The services that keep WaOS's apps.
#[derive(Clone)]
pub(crate) struct AppServices {
    pub(crate) running: Arc<RunningApps>,
    pub(crate) packages: Arc<PackageManager>,
    pub(crate) permissions: Permissions,
    pub(crate) starter: Arc<AppStarter>,
    pub(crate) engine: Arc<WaOS>,
    pub(crate) environments: Arc<ProgramEnvironments>,
    pub(crate) places: ProgramPlaces,
    /// What each app writes, which the dev server's attaches read; a phone has no dev server yet.
    #[cfg(desktop)]
    pub(crate) outputs: Arc<LaunchOutputs>,
}

impl AppServices {
    /// The services over the apps `packages` installs, which `permissions` say what each may use
    /// of, running among `running` on `engine`, each with what `environments` gives it, its views
    /// going where `places` says.
    pub(crate) fn new(
        running: Arc<RunningApps>,
        packages: Arc<PackageManager>,
        permissions: Permissions,
        engine: Arc<WaOS>,
        environments: Arc<ProgramEnvironments>,
        places: ProgramPlaces,
    ) -> AppServices {
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
            permissions,
            starter,
            engine,
            environments,
            places,
            #[cfg(desktop)]
            outputs,
        }
    }
}

/// What WaOS keeps of its apps, each with the domain that owns it: what is installed, what the
/// user lets each app use, and the sources with each app's own folders and activity.
pub(crate) struct Domains {
    pub(crate) packages: Arc<PackageManager>,
    pub(crate) permissions: Permissions,
    pub(crate) store: Store,
}

/// The domains over `folder`, the store's directory, opened in the order each needs the other:
/// the package manager, compiling with `compiler` and stopping apps among `running`; Permissions,
/// asking the user through `prompts` and following the package manager's record; and the store, reading sources through `proxy`, recording as Permissions
/// say, and forgetting each app's activity as the package manager removes it, on a thread of its
/// own.
pub(crate) fn open_domains(
    folder: &Path,
    compiler: Arc<dyn CodeCompiler>,
    running: &Arc<RunningApps>,
    proxy: SourceProxy,
    prompts: Arc<dyn Prompts>,
) -> Result<Domains> {
    let packages = PackageManager::open(folder, compiler, Arc::clone(running) as Arc<dyn StopApp>)
        .with_context(|| format!("opening the apps installed at {}", folder.display()))?;
    let packages = Arc::new(packages);
    let permissions = Permissions::open(
        Arc::new(SettingsFiles::new(folder)),
        prompts,
        Arc::clone(&packages) as Arc<dyn InstalledApps>,
    );
    let store = Store::open(folder, proxy, permissions.clone())
        .with_context(|| format!("opening the store at {}", folder.display()))?;
    let mut removals = packages.removals();
    let forgetting = store.clone();
    std::thread::Builder::new()
        .name("removals".to_owned())
        .spawn(move || {
            // It ends with the package manager.
            while let Some(id) = removals.blocking_recv() {
                forgetting.forget(&id);
            }
        })
        .context("starting the thread that follows removals")?;
    Ok(Domains {
        packages,
        permissions,
        store,
    })
}

/// The engine's compiler, as the package manager is handed it: a type of the app's own, since
/// neither the engine nor the package manager knows the other.
pub(crate) struct EngineCompiler(pub(crate) Compiler);

impl CodeCompiler for EngineCompiler {
    fn compile(&self, component: &Path, code: &Path) -> Result<()> {
        self.0.compile(component, code)
    }

    fn is_usable(&self, code: &Path) -> bool {
        self.0.is_compiled(code)
    }
}
