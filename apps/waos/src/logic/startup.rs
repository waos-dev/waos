//! WaOS's one startup, which the desktop app, the headless entry and the tests share: the domains
//! put together in the order each needs the others, each handed what it needs as it is made. The
//! mailbox the package manager stops an app's instance through first (`app_instances`), so
//! neither end needs the other, and the watch of the instances running, which the app instances
//! write and the shell's parts read; the domains over the store ([`open_domains`]): the package
//! manager, compiling installs and putting its stops in the mailbox, Permissions, asking through
//! the prompts and following the package manager's record, the sources, app storage and the
//! activity record; then the engine, over the shell made from those, keeping its days in the
//! activity record; what each app runs with (`environments`); and the one owner of what runs,
//! reading the mailbox (`app_instances`), with where an app's views go (`program_places`), which
//! hands an inspector the apps on this computer with its instance (`inspected_programs`).
//!
//! What differs between one WaOS and another is passed in: where each domain keeps what it owns
//! (`stores`), where the apps' own folders are, and how it compiles and reaches the network
//! ([`Setup`]), what asks the user, and the shell's parts with
//! this computer's capabilities ([`Outside`]), made from the domains opened before the engine
//! ([`BeforeTheEngine`]). Tests pass the tests' compiler, their prompts and their shells.
//!
//! The invariant: each domain is made once, here, from the others it needs, so no domain is handed
//! another after it is made; the package manager is made before the engine, since the shell's
//! parts read what it installs, and so the compiler the package manager is handed is made before
//! the engine; and the app instances are made last, from the package manager, Permissions and the
//! engine.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use waos::{
    AudioDevices, Compiler, EngineTime, FreezePolicy, HttpProxy, PlatformState, Shell, WaOS,
    WaOSConfig,
};
use waos_local::LocalCapabilities;
use waos_services::activity::ActivityRecord;
use waos_services::app_storage::AppStorage;
use waos_services::packages::{CodeCompiler, InstalledApps, PackageManager};
use waos_services::permissions::{Permissions, Prompts};
use waos_services::sources::{SourceProxy, Sources};

use tokio::sync::watch;

use super::app_instances::{AppInstances, RunningInstances, StopMailbox, stop_mailbox};
use super::environments::ProgramEnvironments;
use super::program_places::{ProgramPlace, ProgramPlaces};
use super::stores::Stores;

/// Where a WaOS keeps what it owns, and how it compiles, reaches the network and keeps time.
pub(crate) struct Setup {
    /// Where each domain keeps what it owns.
    pub(crate) stores: Stores,
    /// The directory each app's own data and cache folders are made in, the first time it runs
    /// (`AppStorage`).
    pub(crate) app_folders: PathBuf,
    /// Where each running app's `/tmp` is made (`WaOSConfig::temporary_folders`).
    pub(crate) temporary_folders: PathBuf,
    /// What compiles the apps' code, which the engine maps.
    pub(crate) compiler: Compiler,
    /// What the package manager compiles an install with: the engine's compiler, or the tests'.
    pub(crate) install_compiler: Arc<dyn CodeCompiler>,
    /// How the sources reach the network.
    pub(crate) source_proxy: SourceProxy,
    /// How the apps' HTTP reaches the network.
    pub(crate) http_proxy: HttpProxy,
    /// When an app out of sight is frozen.
    pub(crate) freeze: FreezePolicy,
    /// What moves the engine's time.
    pub(crate) time: EngineTime,
    /// Where an app's views go unless it was opened somewhere else.
    pub(crate) usual_place: ProgramPlace,
    /// Where WaOS's own tasks run: the app instances' answers to the package manager's stops, and
    /// their waits for each instance's end.
    pub(crate) runtime: tokio::runtime::Handle,
}

/// What the engine's shell is made from: the domains opened before the engine, and the instances
/// running, which the app instances made after it write.
pub(crate) struct BeforeTheEngine<'a> {
    pub(crate) packages: &'a Arc<PackageManager>,
    pub(crate) permissions: &'a Permissions,
    pub(crate) places: &'a ProgramPlaces,
    pub(crate) running: &'a watch::Receiver<RunningInstances>,
}

/// What the engine runs over: the shell's parts, the machine as it starts, and this computer's
/// capabilities, which every app runs with; and `own`, what the caller made with them and keeps.
pub(crate) struct Outside<S> {
    pub(crate) shell: Shell,
    pub(crate) platform: PlatformState,
    pub(crate) capabilities: LocalCapabilities,
    /// The sound devices every app plays on and records from: the capabilities' own, unless a
    /// test gives others.
    pub(crate) audio: Option<Arc<dyn AudioDevices>>,
    pub(crate) own: S,
}

impl<S> Outside<S> {
    /// The engine over `shell`, on the machine `platform` says, its apps running with
    /// `capabilities`, their sound devices among them; with `own` kept.
    pub(crate) fn new(
        shell: Shell,
        platform: PlatformState,
        capabilities: LocalCapabilities,
        own: S,
    ) -> Outside<S> {
        Outside {
            shell,
            platform,
            audio: capabilities.environment().audio,
            capabilities,
            own,
        }
    }
}

/// WaOS's domains, put together: destructured by whoever started WaOS, which hands each part to
/// what uses it, never this whole.
pub(crate) struct Started {
    pub(crate) instances: Arc<AppInstances>,
    pub(crate) packages: Arc<PackageManager>,
    pub(crate) permissions: Permissions,
    pub(crate) sources: Sources,
    /// Each app's own folders, which only what is made here is handed in the app; a test reads
    /// them.
    #[cfg_attr(not(test), expect(dead_code, reason = "a test's alone"))]
    pub(crate) storage: AppStorage,
    /// What each app did, which the engine keeps its days in; a test reads it.
    #[cfg_attr(not(test), expect(dead_code, reason = "a test's alone"))]
    pub(crate) activity: ActivityRecord,
    pub(crate) engine: Arc<WaOS>,
    pub(crate) environments: Arc<ProgramEnvironments>,
    pub(crate) places: ProgramPlaces,
}

/// Starts WaOS as `setup` says: the mailbox of the package manager's stops and the watch of the
/// instances running; the prompts `prompts` makes, with what the caller keeps of them; the domains
/// over the store; the engine over what `outside` makes of those; and the app instances. Answers
/// the domains, and what `outside` kept.
pub(crate) fn start<P, S>(
    setup: Setup,
    prompts: impl FnOnce() -> (Arc<dyn Prompts>, P),
    outside: impl FnOnce(BeforeTheEngine<'_>, P) -> Result<Outside<S>>,
) -> Result<(Started, S)> {
    let (stops, stop_requests) = stop_mailbox();
    let (running, read_running) = watch::channel(RunningInstances::new());
    let (prompts, prompted) = prompts();
    let domains = open_domains(
        setup.stores,
        &setup.app_folders,
        setup.install_compiler,
        stops,
        setup.source_proxy,
        prompts,
    )?;
    let places = ProgramPlaces::new(setup.usual_place);
    let before = BeforeTheEngine {
        packages: &domains.packages,
        permissions: &domains.permissions,
        places: &places,
        running: &read_running,
    };
    let outside = outside(before, prompted)?;
    let config = WaOSConfig {
        freeze: setup.freeze,
        compiler: setup.compiler,
        http_proxy: setup.http_proxy,
        time: setup.time,
        temporary_folders: setup.temporary_folders,
        audit: Some(Arc::new(domains.activity.clone())),
    };
    let engine =
        WaOS::new(config, outside.shell, outside.platform).context("starting the engine")?;
    let engine = Arc::new(engine);
    let environments = ProgramEnvironments::new(
        outside.capabilities,
        outside.audio,
        domains.permissions.clone(),
        domains.storage.clone(),
    );
    let instances = AppInstances::new(
        &domains,
        Arc::clone(&engine),
        Arc::clone(&environments),
        places.clone(),
        stop_requests,
        running,
        setup.runtime,
    );
    let Domains {
        packages,
        permissions,
        sources,
        storage,
        activity,
    } = domains;
    let started = Started {
        instances,
        packages,
        permissions,
        sources,
        storage,
        activity,
        engine,
        environments,
        places,
    };
    Ok((started, outside.own))
}

/// What WaOS keeps of its apps over its stores, each with the domain that owns it: what is
/// installed, what the user lets each app use, the sources, each app's own folders, and what each
/// app did.
pub(crate) struct Domains {
    pub(crate) packages: Arc<PackageManager>,
    pub(crate) permissions: Permissions,
    pub(crate) sources: Sources,
    pub(crate) storage: AppStorage,
    pub(crate) activity: ActivityRecord,
}

/// The domains over `stores`, opened in the order each needs the other: the package manager,
/// compiling with `compiler` and stopping an app's instance through `stops`; Permissions, asking the user
/// through `prompts` and following the package manager's record; the sources, read through
/// `proxy`; app storage, making the apps' own folders in `app_folders`; and the activity record,
/// recording as Permissions say, of the apps the package manager holds. The first steps of
/// [`start`], which a test that never runs an app takes alone.
pub(crate) fn open_domains(
    stores: Stores,
    app_folders: &Path,
    compiler: Arc<dyn CodeCompiler>,
    stops: StopMailbox,
    proxy: SourceProxy,
    prompts: Arc<dyn Prompts>,
) -> Result<Domains> {
    let packages = Arc::new(PackageManager::new(
        stores.installed_apps,
        compiler,
        Arc::new(stops),
    ));
    let permissions = Permissions::open(
        stores.settings,
        prompts,
        Arc::clone(&packages) as Arc<dyn InstalledApps>,
    );
    let sources = Sources::open(stores.sources, proxy).context("opening the sources")?;
    let activity = ActivityRecord::open(
        stores.activity,
        permissions.clone(),
        Arc::clone(&packages) as Arc<dyn InstalledApps>,
    )
    .context("opening the activity record")?;
    Ok(Domains {
        packages,
        permissions,
        sources,
        storage: AppStorage::new(app_folders),
        activity,
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

/// What a test's WaOS is set up with, the stand-ins for the slow outside.
#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use waos_services::permissions::NoPrompts;

    use super::*;
    use crate::logic::test_installs;

    impl Setup {
        /// A test's WaOS over `stores`, the apps' own folders and their `/tmp` in `folder`: the
        /// tests' compiler, compiling each component once in a test run (`test_installs`), no
        /// proxy, the real time, its apps in windows, and its tasks on `runtime`.
        pub(crate) fn for_tests(
            folder: &Path,
            stores: Stores,
            runtime: tokio::runtime::Handle,
        ) -> Setup {
            Setup {
                stores,
                app_folders: folder.join("store"),
                temporary_folders: folder.join("tmp"),
                compiler: Compiler::new(false).expect("a compiler"),
                install_compiler: Arc::new(test_installs::test_compiler()),
                source_proxy: SourceProxy::None,
                http_proxy: HttpProxy::None,
                freeze: FreezePolicy::default(),
                time: EngineTime::Real,
                usual_place: ProgramPlace::Windows,
                runtime,
            }
        }

        /// A test's WaOS that keeps everything in memory, with nothing installed, as
        /// [`Setup::for_tests`] sets it up in `folder`, its tasks on `runtime`.
        pub(crate) fn in_memory(folder: &Path, runtime: tokio::runtime::Handle) -> Setup {
            Setup::for_tests(folder, Stores::in_memory(Vec::new()), runtime)
        }

        /// A test's WaOS that keeps everything in files in `folder`'s `store`, as
        /// [`Setup::for_tests`] sets it up there, its tasks on `runtime`.
        pub(crate) fn on_files(folder: &Path, runtime: tokio::runtime::Handle) -> Setup {
            let stores = Stores::files(&folder.join("store")).expect("the stores open");
            Setup::for_tests(folder, stores, runtime)
        }
    }

    /// A test's WaOS started as `setup` says, with nobody to ask, over the shell and the machine
    /// `outside` makes of the domains before the engine, its apps running with this computer's
    /// capabilities.
    pub(crate) fn start_asking_nobody(
        setup: Setup,
        outside: impl FnOnce(BeforeTheEngine<'_>) -> (Shell, PlatformState),
    ) -> Started {
        let prompts = || (Arc::new(NoPrompts) as Arc<dyn Prompts>, ());
        let (started, ()) = start(setup, prompts, |before, ()| {
            let (shell, platform) = outside(before);
            Ok(Outside::new(shell, platform, LocalCapabilities::new(), ()))
        })
        .expect("WaOS starts");
        started
    }
}
