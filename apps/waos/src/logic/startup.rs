//! WaOS's one startup, which the desktop app, the headless entry and the tests share: the domains
//! put together in the order each needs the others, each handed what it needs as it is made. The
//! apps running (`running_apps`) first, which the prompts find a question's app among; the domains
//! over the store ([`open_domains`]): the package manager, compiling installs and stopping apps
//! among the running ones, Permissions, asking through the prompts and following the package
//! manager's record, the sources, app storage and the activity record; then the engine, over the
//! shell made from those, keeping its days in the activity record; what each app runs with
//! (`environments`); and the one way to start an installed app (`app_starter`), where its views go
//! (`program_places`), which hands an inspector the apps on this computer with its run
//! (`inspected_programs`).
//!
//! What differs between one WaOS and another is passed in: where it keeps its files and how it
//! compiles and reaches the network ([`Setup`]), what asks the user, and the shell's parts with
//! this computer's capabilities ([`Outside`]), made from the domains opened before the engine
//! ([`BeforeTheEngine`]). Tests pass the tests' compiler, their prompts and their shells.
//!
//! The invariant: each domain is made once, here, from the others it needs, so no domain is handed
//! another after it is made; the running apps and the package manager are made before the engine,
//! since the package manager stops apps among the running ones and the shell's parts read what it
//! installs, and so the compiler the package manager is handed is made before the engine.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use waos::{
    AudioDevices, Compiler, EngineTime, FreezePolicy, HttpProxy, PlatformState, Shell, WaOS,
    WaOSConfig,
};
use waos_local::LocalCapabilities;
use waos_services::activity::{ActivityFiles, ActivityRecord};
use waos_services::app_storage::AppStorage;
use waos_services::packages::{
    CodeCompiler, InstalledApps, InstalledAppsFiles, PackageManager, StopApp,
};
use waos_services::permissions::{Permissions, Prompts, SettingsFiles};
use waos_services::sources::{SourceProxy, Sources, SourcesFile};

use super::app_starter::AppStarter;
use super::environments::ProgramEnvironments;
use super::program_places::{ProgramPlace, ProgramPlaces};
use super::running_apps::RunningApps;

/// Where a WaOS keeps its files, and how it compiles, reaches the network and keeps time.
pub(crate) struct Setup {
    /// The store's directory, which the domains keep their files in.
    pub(crate) store: PathBuf,
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
}

/// What the engine's shell is made from: the domains opened before the engine.
pub(crate) struct BeforeTheEngine<'a> {
    pub(crate) packages: &'a Arc<PackageManager>,
    pub(crate) permissions: &'a Permissions,
    pub(crate) places: &'a ProgramPlaces,
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
    pub(crate) running: Arc<RunningApps>,
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
    pub(crate) starter: Arc<AppStarter>,
}

/// Starts WaOS as `setup` says: the running apps; the prompts `prompts` makes, finding a
/// question's app among them, with what the caller keeps of them; the domains over the store; the
/// engine over what `outside` makes of those; and the app starter. Answers the domains, and what
/// `outside` kept.
pub(crate) fn start<P, S>(
    setup: Setup,
    prompts: impl FnOnce(&Arc<RunningApps>) -> (Arc<dyn Prompts>, P),
    outside: impl FnOnce(BeforeTheEngine<'_>, P) -> Result<Outside<S>>,
) -> Result<(Started, S)> {
    let running = Arc::new(RunningApps::default());
    let (prompts, prompted) = prompts(&running);
    let Domains {
        packages,
        permissions,
        sources,
        storage,
        activity,
    } = open_domains(
        &setup.store,
        setup.install_compiler,
        &running,
        setup.source_proxy,
        prompts,
    )?;
    let places = ProgramPlaces::new(setup.usual_place);
    let before = BeforeTheEngine {
        packages: &packages,
        permissions: &permissions,
        places: &places,
    };
    let outside = outside(before, prompted)?;
    let config = WaOSConfig {
        freeze: setup.freeze,
        compiler: setup.compiler,
        http_proxy: setup.http_proxy,
        time: setup.time,
        temporary_folders: setup.temporary_folders,
        audit: Some(Arc::new(activity.clone())),
    };
    let engine =
        WaOS::new(config, outside.shell, outside.platform).context("starting the engine")?;
    let engine = Arc::new(engine);
    let environments = ProgramEnvironments::new(
        outside.capabilities,
        outside.audio,
        permissions.clone(),
        storage.clone(),
    );
    let starter = Arc::new(AppStarter::new(
        Arc::clone(&running),
        Arc::clone(&packages),
        permissions.clone(),
        storage.clone(),
        Arc::clone(&engine),
        Arc::clone(&environments),
        places.clone(),
    ));
    let started = Started {
        running,
        packages,
        permissions,
        sources,
        storage,
        activity,
        engine,
        environments,
        places,
        starter,
    };
    Ok((started, outside.own))
}

/// What WaOS keeps of its apps over the store, each with the domain that owns it: what is
/// installed, what the user lets each app use, the sources, each app's own folders, and what each
/// app did.
pub(crate) struct Domains {
    pub(crate) packages: Arc<PackageManager>,
    pub(crate) permissions: Permissions,
    pub(crate) sources: Sources,
    pub(crate) storage: AppStorage,
    pub(crate) activity: ActivityRecord,
}

/// The domains over `folder`, the store's directory, opened in the order each needs the other:
/// the package manager, compiling with `compiler` and stopping apps among `running`; Permissions,
/// asking the user through `prompts` and following the package manager's record; the sources,
/// read through `proxy`; app storage; and the activity record, recording as Permissions say, of
/// the apps the package manager holds. The first steps of [`start`], which a test that never runs
/// an app takes alone.
pub(crate) fn open_domains(
    folder: &Path,
    compiler: Arc<dyn CodeCompiler>,
    running: &Arc<RunningApps>,
    proxy: SourceProxy,
    prompts: Arc<dyn Prompts>,
) -> Result<Domains> {
    let installed = InstalledAppsFiles::open(folder)
        .with_context(|| format!("opening the apps installed at {}", folder.display()))?;
    let packages = Arc::new(PackageManager::new(
        Arc::new(installed),
        compiler,
        Arc::clone(running) as Arc<dyn StopApp>,
    ));
    let permissions = Permissions::open(
        Arc::new(SettingsFiles::new(folder)),
        prompts,
        Arc::clone(&packages) as Arc<dyn InstalledApps>,
    );
    let sources = Sources::open(
        Arc::new(SourcesFile::new(folder)),
        &folder.join("pictures"),
        proxy,
    )
    .context("opening the sources")?;
    let activity = ActivityRecord::open(
        Arc::new(ActivityFiles::new(folder)),
        permissions.clone(),
        Arc::clone(&packages) as Arc<dyn InstalledApps>,
    )
    .context("opening the activity record")?;
    Ok(Domains {
        packages,
        permissions,
        sources,
        storage: AppStorage::new(folder),
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
        /// A test's WaOS in `folder`: the tests' compiler, compiling each component once in a
        /// test run (`test_installs`), no proxy, the real time, and its apps in windows.
        pub(crate) fn for_tests(folder: &Path) -> Setup {
            Setup {
                store: folder.join("store"),
                temporary_folders: folder.join("tmp"),
                compiler: Compiler::new(false).expect("a compiler"),
                install_compiler: Arc::new(test_installs::test_compiler()),
                source_proxy: SourceProxy::None,
                http_proxy: HttpProxy::None,
                freeze: FreezePolicy::default(),
                time: EngineTime::Real,
                usual_place: ProgramPlace::Windows,
            }
        }
    }

    /// A test's WaOS started as `setup` says, with nobody to ask, over the shell and the machine
    /// `outside` makes of the domains before the engine, its apps running with this computer's
    /// capabilities.
    pub(crate) fn start_asking_nobody(
        setup: Setup,
        outside: impl FnOnce(BeforeTheEngine<'_>) -> (Shell, PlatformState),
    ) -> Started {
        let prompts = |_: &Arc<RunningApps>| (Arc::new(NoPrompts) as Arc<dyn Prompts>, ());
        let (started, ()) = start(setup, prompts, |before, ()| {
            let (shell, platform) = outside(before);
            Ok(Outside::new(shell, platform, LocalCapabilities::new(), ()))
        })
        .expect("WaOS starts");
        started
    }
}
