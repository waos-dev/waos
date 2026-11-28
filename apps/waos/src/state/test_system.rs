//! The app over a system of Waft's test platform, for what the programs reach of the system
//! through it: the opener, the notifications, the compositor, the players, the reading of media,
//! the frame readers and the cameras the test gives the platform, served to the engine as the app serves them, with the
//! app's indicator of who records, and the clicks on the notifications going to the library, over
//! a store in memory, which the test fills with the apps its build made; and an engine over them,
//! on time that stands still, with no windows and nobody to ask. The test plays the host: it
//! runs the app's tasks, and reports through Waft's client what the system does.

use crate::state::test_turns::{Turn, turn_until, wake_once};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use waft::ui::Dispatcher;

use futures::channel::mpsc::UnboundedReceiver;
use tempfile::TempDir;
use waft::ui::{Platform, PlatformRef};
use waft::{Entity, Shell, Subscription};
use waft_test::TestPlatform;
use waos::{
    AppInstance, EngineTime, ManualTime, OutputStream, ProgramOutput, RecordingDevice,
    ShellCompositor, ShellNotifications, ShellRecordingIndicator, ShellVideoFrameReaders,
};
use waos_dev_protocol::{OutputStream as AttachedStream, RunEnd, RunEvent};
use waos_package::ProgramId;
use waos_program_window::WaftCompositor;
use waos_services::app_storage::AppStorage;
use waos_services::packages::{InstalledAppsInMemory, InstalledAppsStore};

use super::cameras::Cameras;
use super::image_codecs::ImageCodecs;
use super::library::{
    Library, LibraryDomains, ProgramsOpen, WantsToBeSeen, open_library, take_notification_clicks,
    work,
};
use super::media_assets::MediaAssets;
use super::media_players::MediaPlayers;
use super::notifications::Notifications;
use super::opener::Opener;
use super::shells::this_system;
use super::video_decoders::VideoDecoders;
use crate::logic::app_instances::{AppInstances, OpenConfiguration, OpenPlace, OpenedFor};
use crate::logic::frame_readers::ReadersOnRequest;
use crate::logic::package_copies::PackageCopies;
use crate::logic::recording_indicator::{RecordingChanged, RecordingIndicatorOnRequest};
use crate::logic::startup::tests::start_asking_nobody;
use crate::logic::startup::{self, Setup};
use crate::logic::stores::Stores;
use crate::logic::test_engine::{self, NoWindows};
use crate::logic::test_installs;

/// `path` as the engine hands it to the system: its links resolved, named as the system's apps
/// name it (`waos::machine_paths::plain`).
pub(crate) fn real_path(path: &Path) -> PathBuf {
    waos::machine_paths::plain(std::fs::canonicalize(path).expect("resolving a test's path"))
}

pub(crate) struct TestSystem {
    pub(crate) shell: Shell,
    /// The platform the app runs on, which tells the test when a task wakes the app.
    platform: Rc<TestPlatform>,
    /// The app's notifications as the engine reaches them, for a test to call as the engine does.
    pub(crate) notifications: Option<Arc<dyn ShellNotifications>>,
    pub(crate) storage: AppStorage,
    pub(crate) permissions: waos_services::permissions::Permissions,
    packages: Arc<waos_services::packages::PackageManager>,
    /// The installed apps, which the test fills with the apps its build made.
    installed: Arc<InstalledAppsInMemory>,
    pub(crate) runs: Arc<AppInstances>,
    /// Where the app instances' tasks run, and an open is waited for.
    runtime: tokio::runtime::Handle,
    pub(crate) library: Entity<Library>,
    /// How many times the library asked to be seen.
    asked_to_be_seen: Rc<Cell<usize>>,
    /// What the app's indicator of who records was told, as it arrives, and what arrived so far.
    recording_changes: RefCell<UnboundedReceiver<RecordingChanged>>,
    recording_told: RefCell<Vec<(String, RecordingDevice, bool)>>,
    _seen: Subscription,
    /// The engine's time, which stands still, so no program freezes for being out of sight.
    _time: ManualTime,
    /// The test's own folder, removed with the system.
    _folder: TempDir,
}

impl TestSystem {
    /// The app over `platform`, with an engine whose shell has the app's opener, notifications,
    /// compositor, players, reading of media, frame readers and cameras where `platform` has them, and the app's
    /// indicator of who records.
    pub(crate) fn new(platform: TestPlatform) -> TestSystem {
        let folder = tempfile::tempdir().unwrap();
        let copies = PackageCopies::clear(folder.path().join("package-copies")).unwrap();
        let opener = Opener::of(&platform);
        let notifications = Notifications::of(&platform);
        let shell_notifications = notifications.as_ref().map(Notifications::shell);
        let media_players = MediaPlayers::of(&platform);
        let media_assets = MediaAssets::of(&platform);
        let image_codecs = ImageCodecs::of(&platform);
        let video_decoders = VideoDecoders::of(&platform);
        let cameras = Cameras::of(&platform);
        let (recording_indicator, recording_changes) = RecordingIndicatorOnRequest::new();
        let mut shell = test_engine::shell(Arc::new(NoWindows));
        shell.opener = opener.as_ref().map(Opener::shell);
        shell.notifications = shell_notifications.clone();
        shell.media_players = media_players.as_ref().map(MediaPlayers::shell);
        shell.media_assets = media_assets.as_ref().map(MediaAssets::shell);
        shell.image_codecs = image_codecs.as_ref().map(ImageCodecs::shell);
        shell.gpus = video_decoders.as_ref().map(VideoDecoders::shell);
        shell.cameras = cameras.as_ref().map(Cameras::shell);
        shell.simulated_cameras = this_system::simulated_cameras(shell.image_codecs.clone());
        shell.recording_indicator =
            Some(Arc::new(recording_indicator) as Arc<dyn ShellRecordingIndicator>);
        shell.video_frame_readers = platform
            .video_frame_readers()
            .map(|_| Arc::new(ReadersOnRequest) as Arc<dyn ShellVideoFrameReaders>);
        shell.compositor = platform.compositor().map(|compositor| {
            Arc::new(WaftCompositor::new(compositor)) as Arc<dyn ShellCompositor>
        });
        let time = ManualTime::new();
        let work = work().unwrap();
        let installed = Arc::new(InstalledAppsInMemory::default());
        let stores = Stores {
            installed_apps: Arc::clone(&installed) as Arc<dyn InstalledAppsStore>,
            ..Stores::in_memory(Vec::new())
        };
        let runtime = work.handle().clone();
        let setup = Setup {
            time: EngineTime::Manual(time.clone()),
            ..Setup::for_tests(folder.path(), stores, runtime.clone())
        };
        let started = start_asking_nobody(setup, |_before| (shell, test_engine::no_screens()));
        let domains = LibraryDomains::of(&started);
        let startup::Started {
            instances: runs,
            packages,
            permissions,
            storage,
            engine,
            ..
        } = started;
        let started = Rc::new(RefCell::new(None));
        let served_packages = Arc::clone(&packages);
        let kept = Rc::clone(&started);
        let suspended = Arc::clone(&engine);
        let platform = Rc::new(platform);
        let shell = Shell::new(Rc::clone(&platform) as PlatformRef, move |app| {
            super::app_suspension::follow_app_suspension(app, suspended);
            if let Some(opener) = opener {
                opener.serve(app);
            }
            let posted_as_programs =
                notifications.map(|notifications| notifications.serve(app, served_packages));
            if let Some(video_decoders) = video_decoders {
                video_decoders.serve(app);
            }
            if let Some(media_players) = media_players {
                media_players.serve(app);
            }
            if let Some(media_assets) = media_assets {
                media_assets.serve(app);
            }
            if let Some(image_codecs) = image_codecs {
                image_codecs.serve(app);
            }
            if let Some(cameras) = cameras {
                cameras.serve(app);
            }
            let library = open_library(app, domains, copies, ProgramsOpen::InWindows, None, work);
            take_notification_clicks(app, &library, posted_as_programs);
            *kept.borrow_mut() = Some(library);
        });
        let library = started.borrow_mut().take().expect("the app started");
        let asked_to_be_seen = Rc::new(Cell::new(0));
        let counted = Rc::clone(&asked_to_be_seen);
        let seen = shell
            .app()
            .subscribe(&library, move |_, _: &WantsToBeSeen| {
                counted.set(counted.get() + 1);
            });
        TestSystem {
            shell,
            platform,
            notifications: shell_notifications,
            storage,
            permissions,
            packages,
            installed,
            runs,
            runtime,
            library,
            asked_to_be_seen,
            recording_changes: RefCell::new(recording_changes),
            recording_told: RefCell::default(),
            _seen: seen,
            _time: time,
            _folder: folder,
        }
    }

    /// Installs the test program at `component` as `id`, named `name`: the store is filled with the
    /// app the test's build made (`logic::test_installs`).
    pub(crate) fn install(&self, component: &str, id: &str, name: &str) {
        self.installed
            .put(test_installs::built_app(component, id, name));
    }

    /// Runs the test program at `component` as the app `id`, installed first where it is not, its
    /// instance opened as a developer opens it, with the store's settings for it and its own
    /// folders, what it writes to its standard output heard into `output`, as an attach hears it.
    pub(crate) fn run_as(&self, component: &str, id: &str, output: &Arc<Lines>) -> AppInstance {
        let app: ProgramId = id.parse().expect("an id");
        if self.packages.installed_program(&app).is_none() {
            self.install(component, id, id);
        }
        let host = self.platform.dispatcher();
        run_heard(&self.runs, &self.runtime, &app, output, host)
    }

    /// What the library says went wrong last.
    pub(crate) fn problem(&self) -> Option<String> {
        self.library
            .read(&self.shell.app())
            .problem()
            .map(str::to_owned)
    }

    pub(crate) fn asked_to_be_seen(&self) -> usize {
        self.asked_to_be_seen.get()
    }

    /// What the app's indicator of who records was told so far, in order: which program, with
    /// what, and whether it records.
    pub(crate) fn recording_told(&self) -> Vec<(String, RecordingDevice, bool)> {
        let mut changes = self.recording_changes.borrow_mut();
        let mut told = self.recording_told.borrow_mut();
        while let Ok(change) = changes.try_recv() {
            told.push((
                change.program.name().to_owned(),
                change.device,
                change.recording,
            ));
        }
        told.clone()
    }

    /// Runs the app's tasks as they wake until `done`, failing after `WITHIN`.
    pub(crate) fn run_tasks_until(&self, what: &str, done: impl Fn() -> bool) {
        turn_until(&self.platform, what, || {
            self.shell.cell().checkpoint();
            if done() { Turn::Done } else { Turn::Idle }
        });
    }

    /// Runs the app's tasks until the engine says `running` is frozen.
    pub(crate) fn run_tasks_until_frozen(&self, running: &AppInstance) {
        let frozen = wake_once(running.frozen(), self.platform.dispatcher());
        self.run_tasks_until("the app is frozen", || frozen.load(Ordering::Acquire));
    }
}

impl Drop for TestSystem {
    fn drop(&mut self) {
        self.runtime.block_on(self.runs.stop_all());
    }
}

/// Opens the installed app `app` with `instances` as a developer opens it, waited for on `runtime`,
/// what it writes to its standard output heard into `output`, as an attach hears it, and `host`
/// woken as it writes and once it ends.
pub(crate) fn run_heard(
    instances: &Arc<AppInstances>,
    runtime: &tokio::runtime::Handle,
    app: &ProgramId,
    output: &Arc<Lines>,
    host: Arc<dyn Dispatcher>,
) -> AppInstance {
    let configuration = OpenConfiguration {
        replaces_running: true,
        place: OpenPlace::AsItWas,
        ..OpenConfiguration::new(OpenedFor::Development)
    };
    let opened = runtime
        .block_on(instances.open(app, configuration))
        .expect("the app opens");
    let running = opened.instance().clone();
    let attached = (instances.outputs().of(app)).expect("its output").attach();
    output.wake(Arc::clone(&host));
    let heard = Arc::clone(output);
    runtime.spawn(async move {
        while let Some(event) = attached.next().await {
            match event {
                RunEvent::Output {
                    stream: AttachedStream::Stdout,
                    bytes,
                } => heard.write(&bytes),
                RunEvent::Ended(end) => heard.end(end),
                _ => {}
            }
        }
    });
    wake_once(running.ended(), host);
    running
}

/// What a program wrote to its standard output, line by line: as its output, or as an attach to
/// its instance hears it, with how it ended once the attach heard, after the last line.
#[derive(Default)]
pub(crate) struct Lines {
    bytes: Mutex<Vec<u8>>,
    end: Mutex<Option<RunEnd>>,
    /// The test's host, woken as the program writes, so a test turning its app looks again.
    host: OnceLock<Arc<dyn Dispatcher>>,
}

impl Lines {
    /// Wakes `host` from now on as the program writes.
    pub(crate) fn wake(&self, host: Arc<dyn Dispatcher>) {
        let _ = self.host.set(host);
    }

    /// The program wrote `bytes` to its standard output: kept, and the test's host woken.
    fn write(&self, bytes: &[u8]) {
        self.bytes.lock().unwrap().extend_from_slice(bytes);
        if let Some(host) = self.host.get() {
            host.wake_now();
        }
    }

    /// The run ended as `end`, as the attach heard after every line: kept, and the test's host
    /// woken.
    fn end(&self, end: RunEnd) {
        *self.end.lock().unwrap() = Some(end);
        if let Some(host) = self.host.get() {
            host.wake_now();
        }
    }

    /// How the run ended, once the attach heard it, which is after every line it wrote.
    pub(crate) fn ended(&self) -> Option<RunEnd> {
        self.end.lock().unwrap().clone()
    }

    pub(crate) fn has(&self, line: &str) -> bool {
        self.all().iter().any(|written| written == line)
    }

    /// Every line written so far, in order.
    pub(crate) fn all(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.bytes.lock().unwrap())
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl ProgramOutput for Lines {
    fn poll_write(
        &self,
        stream: OutputStream,
        _cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<usize> {
        if matches!(stream, OutputStream::Stdout) {
            self.write(bytes);
        }
        std::task::Poll::Ready(bytes.len())
    }
}
