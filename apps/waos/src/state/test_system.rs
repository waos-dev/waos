//! The app over a system of Waft's test platform, for what the programs reach of the system
//! through it: the opener, the notifications, the compositor, the players, the reading of media,
//! the frame readers and the cameras the test gives the platform, served to the engine as the app serves them, with the
//! app's indicator of who records, and the clicks on the notifications going to the library, over
//! a store in a folder of the test's own, where the test installs programs; and an engine over
//! them, on time that stands still, with no windows and nobody to ask. The test plays the host: it
//! runs the app's tasks, and reports through Waft's client what the system does.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedReceiver;
use tempfile::TempDir;
use waft::ui::{Platform, PlatformRef};
use waft::{Entity, Shell, Subscription};
use waft_test::TestPlatform;
use waos::{
    EngineTime, ManualTime, OutputStream, ProgramEnvironment, ProgramOutput, RecordingDevice,
    RunningProgram, ShellCompositor, ShellNotifications, ShellRecordingIndicator,
    ShellVideoFrameReaders, WaOS,
};
use waos_package::{ProgramId, SigningKey};
use waos_program_window::WaftCompositor;
use waos_services::store::Store;

use super::cameras::Cameras;
use super::image_codecs::ImageCodecs;
use super::library::{
    Library, ProgramsOpen, WantsToBeSeen, open_library, take_notification_clicks,
};
use super::media_assets::MediaAssets;
use super::media_players::MediaPlayers;
use super::notifications::Notifications;
use super::opener::Opener;
use super::video_decoders::VideoDecoders;
use crate::logic::app_services::AppServices;
use crate::logic::environments::ProgramEnvironments;
use crate::logic::frame_readers::ReadersOnRequest;
use crate::logic::opened_package::tests::write_package;
use crate::logic::package_copies::PackageCopies;
use crate::logic::program_places::{ProgramPlace, ProgramPlaces};
use crate::logic::recording_indicator::{RecordingChanged, RecordingIndicatorOnRequest};
use crate::logic::running_apps::{IfRunning, RunningApps};
use crate::logic::test_engine::{self, NoWindows, WITHIN};
use crate::logic::test_installs;

/// `path` as the engine hands it to the system: its links resolved, named as the system's apps
/// name it (`waos::machine_paths::plain`).
pub(crate) fn real_path(path: &Path) -> PathBuf {
    waos::machine_paths::plain(std::fs::canonicalize(path).expect("resolving a test's path"))
}

pub(crate) struct TestSystem {
    pub(crate) shell: Shell,
    engine: Arc<WaOS>,
    /// The app's notifications as the engine reaches them, for a test to call as the engine does.
    pub(crate) notifications: Option<Arc<dyn ShellNotifications>>,
    pub(crate) store: Store,
    pub(crate) permissions: waos_services::permissions::Permissions,
    packages: Arc<waos_services::packages::PackageManager>,
    pub(crate) runs: Arc<RunningApps>,
    pub(crate) library: Entity<Library>,
    /// How many times the library asked to be seen.
    asked_to_be_seen: Rc<Cell<usize>>,
    /// What the app's indicator of who records was told, as it arrives, and what arrived so far.
    recording_changes: RefCell<UnboundedReceiver<RecordingChanged>>,
    recording_told: RefCell<Vec<(String, RecordingDevice, bool)>>,
    _seen: Subscription,
    /// The engine's time, which stands still, so no program freezes for being out of sight.
    _time: ManualTime,
    folder: TempDir,
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
        shell.simulated_cameras = super::simulated_cameras(shell.image_codecs.clone());
        shell.recording_indicator =
            Some(Arc::new(recording_indicator) as Arc<dyn ShellRecordingIndicator>);
        shell.video_frame_readers = platform
            .video_frame_readers()
            .map(|_| Arc::new(ReadersOnRequest) as Arc<dyn ShellVideoFrameReaders>);
        shell.compositor = platform.compositor().map(|compositor| {
            Arc::new(WaftCompositor::new(compositor)) as Arc<dyn ShellCompositor>
        });
        let time = ManualTime::new();
        let engine = Arc::new(test_engine::engine_over(
            shell,
            EngineTime::Manual(time.clone()),
        ));
        let runs = Arc::new(RunningApps::default());
        let crate::logic::app_services::Domains {
            packages,
            permissions,
            sources,
            store,
        } = test_installs::domains(
            &folder.path().join("store"),
            &runs,
            Arc::new(waos_services::permissions::NoPrompts),
        );
        let services = AppServices::new(
            Arc::clone(&runs),
            Arc::clone(&packages),
            permissions.clone(),
            Arc::clone(&engine),
            ProgramEnvironments::new(
                waos_local::LocalCapabilities::new(),
                permissions.clone(),
                store.clone(),
            ),
            ProgramPlaces::new(ProgramPlace::Windows),
        );
        let (served_sources, started) = (sources, Rc::new(RefCell::new(None)));
        let served_packages = Arc::clone(&packages);
        let kept = Rc::clone(&started);
        let suspended = Arc::clone(&engine);
        let shell = Shell::new(Rc::new(platform) as PlatformRef, move |app| {
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
            let library = open_library(
                app,
                served_sources,
                services,
                copies,
                ProgramsOpen::InWindows,
                None,
            )
            .expect("a library");
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
            engine,
            notifications: shell_notifications,
            store,
            permissions,
            packages,
            runs,
            library,
            asked_to_be_seen,
            recording_changes: RefCell::new(recording_changes),
            recording_told: RefCell::default(),
            _seen: seen,
            _time: time,
            folder,
        }
    }

    /// Installs the test program at `component` as `id`, named `name`, from a package signed
    /// with a key of the test's own, its code compiled already (`logic::test_installs`).
    pub(crate) fn install(&self, component: &str, id: &str, name: &str) {
        let packages = self.folder.path().join("packages");
        std::fs::create_dir_all(&packages).unwrap();
        let bytes = std::fs::read(component).unwrap();
        let key = SigningKey::generate();
        let file = write_package(&packages, id, name, "1.0.0", &bytes, &key);
        test_installs::install_package(&self.packages, &file);
    }

    /// Runs the test program at `component` as the program `id`, among the runs, with the store's
    /// settings for it and its own folders, its output into `output`.
    pub(crate) fn run_as(&self, component: &str, id: &str, output: &Arc<Lines>) -> RunningProgram {
        let id: ProgramId = id.parse().expect("an id");
        let program = self
            .engine
            .load(
                Path::new(component),
                &waos_test_programs::code_of(component),
            )
            .expect("the program loads")
            .named(id.as_str());
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(output) as Arc<dyn ProgramOutput>),
            grants: Some(self.permissions.of(&id)),
            folders: Some(self.store.folders(&id).expect("its folders")),
            ..ProgramEnvironment::default()
        };
        let running = self
            .engine
            .run(&program, environment)
            .expect("the program runs");
        let listed = running.clone();
        futures::executor::block_on(self.runs.start(&id, IfRunning::Replace, || Ok(listed)))
            .expect("among the runs");
        running
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

    /// Runs the app's tasks until `done`, failing after [`WITHIN`].
    pub(crate) fn run_tasks_until(&self, what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + WITHIN;
        while !done() {
            assert!(Instant::now() < deadline, "{what} never happened");
            thread::sleep(Duration::from_millis(1));
            self.shell.cell().checkpoint();
        }
    }
}

impl Drop for TestSystem {
    fn drop(&mut self) {
        futures::executor::block_on(self.runs.stop_all());
    }
}

/// What a program wrote to its standard output, line by line.
#[derive(Default)]
pub(crate) struct Lines {
    bytes: Mutex<Vec<u8>>,
}

impl Lines {
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
            self.bytes.lock().unwrap().extend_from_slice(bytes);
        }
        std::task::Poll::Ready(bytes.len())
    }
}
