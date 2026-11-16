//! A shell that shows nothing, for running a program as an app would with no screen: it accepts
//! every view request and answers each at once with a view of its own on its one screen, which
//! refreshes a moment after the engine asks, or no more once a test stops it, keeps the hardware
//! buffer presented last and releases the one before, or holds every one when a test asks, holds a
//! view's presents back when a test asks, as GPU work that does not finish holds a frame, records
//! the composition layers each present names, made by a compositor of its own (`composition`),
//! records what the program asks of its views and windows, holds each question a program puts to its user,
//! each link scheme it asks to open and each settings page it opens until the test answers or
//! leaves it, and records what every program prints, the glyphs the engine draws for it and the
//! frames the engine hands over, which the engine turns into `tracing` events; and holds the
//! clipboard, as the platform does (`clipboard`), the menus, the menu bar, context menus and tray
//! icons (`menus`), the picker of a sound's output and the indicator of who records, beside sound
//! devices of its own (`sound`), the file dialogs (`file_dialogs`), the system's opener (`opener`),
//! its notifications (`notifications`), its players (`media_players`), its reading of media
//! without playing it (`media_assets`), its image codecs (`image_codecs`), its frame readers
//! (`video_frames`) and its cameras (`cameras`), and the picker of a machine (`machines`). It reports to the engine when each
//! refresh's picture is expected on screen, and keeps it. A test that reads a program's lines in
//! the log takes the program's turn first (`turns`).

// Each test uses the part of this it needs.
#![allow(dead_code)]

pub mod cameras;
pub mod clipboard;
pub mod composition;
pub mod file_dialogs;
pub mod hardware_buffer;
pub mod image_codecs;
pub mod machines;
pub mod media_assets;
pub mod media_players;
pub mod menus;
pub mod notifications;
pub mod opener;
pub mod sound;
pub mod turns;
pub mod video_frames;

use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::channel::oneshot;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use waos::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
use waos::bindings::wasdk::platform::screens::{PixelSize, ScreenId};
use waos::bindings::wasdk::platform::system::{Brightness, Locale};
use waos::bindings::wasdk::platform::views::{
    Cursor, Insets, Metrics, OverlayStyle, PixelRect, Placement, PointerButtons, PointerChange,
    PointerDeviceKind, PointerEvent, PointerSignalKind, RequestId, TextEditingValue,
    TextInputConfiguration, TextInputType, ViewError, ViewFocusDirection, ViewFocusState, ViewId,
};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size, TitleBarAction};
use waos::{
    Answer, AnswerFuture, EngineTime, FreezePolicy, HardwareBuffer, HttpProxy, ManualTime,
    OutputStream, PlacedView, PlacedWindow, PlatformState, Presence, Program, ProgramEnd,
    ProgramOutput, ProgramView, Prompts, Question, Role, RunningProgram, Screen,
    SettingsClosedFuture, Shell, ShellAudioOutputPicker, ShellCameras, ShellClipboard,
    ShellCompositionLayer, ShellCompositor, ShellContextMenus, ShellFileDialogs, ShellImageCodecs,
    ShellMachinePicker, ShellMediaAssets, ShellMediaPlayers, ShellMenuBar, ShellNotifications,
    ShellOpener, ShellRecordingIndicator, ShellTextInput, ShellTrayIcons, ShellVideoFrameReaders,
    ShellView, ShellWindow, ShellWindowSizeLimits, Switch, ViewPlacement, ViewRequest, VsyncWaiter,
    WaOS, WaOSConfig, WindowConfig,
};

use cameras::TestCameras;
use clipboard::TestClipboard;
use composition::{PresentedLayer, TestCompositor};
use file_dialogs::TestFileDialogs;
use hardware_buffer::HeldHardwareBuffer;
use image_codecs::TestImageCodecs;
use machines::TestMachinePicker;
use media_assets::TestMediaAssets;
use media_players::TestMediaPlayers;
use menus::TestMenus;
use notifications::TestNotifications;
use opener::TestOpener;
use sound::TestSound;
use video_frames::TestVideoFrameReaders;

/// How long a test waits for a program to do something.
pub const WITHIN: Duration = Duration::from_secs(60);

/// The size of every view the shell places, a Mac window's.
pub const VIEW_WIDTH: u32 = 1600;
pub const VIEW_HEIGHT: u32 = 1000;

/// The one screen the shell reports.
pub const SCREEN: u64 = 1;

/// How long after the engine asks the screen refreshes: a screen at 250 Hz, so the views a
/// program asks frames of in one go are served by one refresh, as on a real screen.
const REFRESH_INTERVAL: Duration = Duration::from_millis(4);

/// An engine over a test shell, set up as [`config`] says, with one screen, light, in American
/// English; and where the views the shell places arrive, as the shell adds them.
pub fn engine() -> (Arc<WaOS>, Arc<TestShell>, Receiver<ProgramView>) {
    engine_with(config())
}

/// [`engine`], freezing programs as `freeze` says, on time the test moves with `time` rather
/// than real time.
pub fn engine_on(
    time: &ManualTime,
    freeze: FreezePolicy,
) -> (Arc<WaOS>, Arc<TestShell>, Receiver<ProgramView>) {
    engine_with(WaOSConfig {
        freeze,
        time: EngineTime::Manual(time.clone()),
        ..config()
    })
}

/// Waits until `program` runs nothing at this moment, its GPU work done
/// (`RunningProgram::idle`), failing after [`WITHIN`].
pub fn wait_until_idle(program: &RunningProgram) {
    let (idle, came) = mpsc::channel();
    let waiting = program.idle();
    thread::spawn(move || {
        futures::executor::block_on(waiting);
        // A test that timed out has dropped the receiver.
        let _ = idle.send(());
    });
    came.recv_timeout(WITHIN)
        .expect("the program comes to run nothing");
}

/// Waits until `program` is frozen (`RunningProgram::frozen`), failing after [`WITHIN`].
pub fn wait_until_frozen(program: &RunningProgram) {
    let (frozen, came) = mpsc::channel();
    let waiting = program.frozen();
    thread::spawn(move || {
        futures::executor::block_on(waiting);
        // A test that timed out has dropped the receiver.
        let _ = frozen.send(());
    });
    came.recv_timeout(WITHIN).expect("the program is frozen");
}

/// Waits until `program` stands at its freeze, stopped by it at an epoch check
/// (`RunningProgram::stopped_at_freeze`), failing after [`WITHIN`].
pub fn wait_until_stopped_at_freeze(program: &RunningProgram) {
    let (stopped, came) = mpsc::channel();
    let waiting = program.stopped_at_freeze();
    thread::spawn(move || {
        futures::executor::block_on(waiting);
        // A test that timed out has dropped the receiver.
        let _ = stopped.send(());
    });
    came.recv_timeout(WITHIN)
        .expect("the program stops at its freeze");
}

/// How the tests' engines are set up unless a test says otherwise: the code cache every test
/// shares, freezing by the default policy, no native unwind information, as an app has it, HTTP
/// through no proxy, whatever the machine's settings, real time, and the programs' `/tmp` in a
/// folder of the tests' own in the system's temporary folder.
pub fn config() -> WaOSConfig {
    WaOSConfig {
        code_cache: PathBuf::from(waos_test_programs::CODE_CACHE),
        freeze: FreezePolicy::default(),
        native_unwind_info: false,
        http_proxy: HttpProxy::None,
        time: EngineTime::Real,
        temporary_folders: std::env::temp_dir().join("waos-tests"),
        audit: None,
        inspected_programs: None,
    }
}

/// [`engine`], set up as `config` says. The shell reports its screen's refreshes to it.
pub fn engine_with(config: WaOSConfig) -> (Arc<WaOS>, Arc<TestShell>, Receiver<ProgramView>) {
    engine_of(config, |_parts| {})
}

/// [`engine_with`], its shell's parts as `adjust` leaves them.
pub fn engine_of(
    config: WaOSConfig,
    adjust: impl FnOnce(&mut Shell),
) -> (Arc<WaOS>, Arc<TestShell>, Receiver<ProgramView>) {
    let (shell, arrivals) = TestShell::new();
    let platform = PlatformState {
        screens: vec![screen()],
        platform_brightness: Brightness::Light,
        caption_style: None,
        locales: vec![Locale {
            language: "en".into(),
            script: None,
            region: Some("US".into()),
        }],
        time_zone: Some("Asia/Tokyo".into()),
    };
    let mut parts = Shell {
        views: Arc::clone(&shell) as Arc<dyn ViewPlacement>,
        vsync: Arc::clone(&shell) as Arc<dyn VsyncWaiter>,
        compositor: Some(Arc::clone(&shell.compositor) as Arc<dyn ShellCompositor>),
        prompts: Arc::clone(&shell) as Arc<dyn Prompts>,
        clipboard: Some(Arc::clone(&shell.clipboard) as Arc<dyn ShellClipboard>),
        menu_bar: Some(Arc::clone(&shell.menus) as Arc<dyn ShellMenuBar>),
        context_menus: Some(Arc::clone(&shell.menus) as Arc<dyn ShellContextMenus>),
        tray_icons: Some(Arc::clone(&shell.menus) as Arc<dyn ShellTrayIcons>),
        audio_output_picker: Some(Arc::clone(&shell.sound) as Arc<dyn ShellAudioOutputPicker>),
        recording_indicator: Some(Arc::clone(&shell.sound) as Arc<dyn ShellRecordingIndicator>),
        file_dialogs: Some(Arc::clone(&shell.file_dialogs) as Arc<dyn ShellFileDialogs>),
        opener: Some(Arc::clone(&shell.opener) as Arc<dyn ShellOpener>),
        notifications: Some(Arc::clone(&shell.notifications) as Arc<dyn ShellNotifications>),
        media_players: Some(Arc::clone(&shell.media_players) as Arc<dyn ShellMediaPlayers>),
        media_assets: Some(Arc::clone(&shell.media_assets) as Arc<dyn ShellMediaAssets>),
        image_codecs: Some(Arc::clone(&shell.image_codecs) as Arc<dyn ShellImageCodecs>),
        video_frame_readers: Some(
            Arc::clone(&shell.video_frame_readers) as Arc<dyn ShellVideoFrameReaders>
        ),
        cameras: Some(Arc::clone(&shell.cameras) as Arc<dyn ShellCameras>),
        simulated_cameras: Some(Arc::clone(&shell.simulated_cameras) as Arc<dyn ShellCameras>),
        machine_picker: Some(Arc::clone(&shell.machine_picker) as Arc<dyn ShellMachinePicker>),
    };
    adjust(&mut parts);
    let engine = Arc::new(WaOS::new(config, parts, platform).expect("an engine"));
    let _ = shell.engine.set(Arc::downgrade(&engine));
    (engine, shell, arrivals)
}

/// The program at `component`, one of `waos_test_programs`'.
pub fn load(engine: &WaOS, component: &str) -> Program {
    engine
        .load(Path::new(component))
        .unwrap_or_else(|error| panic!("loading {component}: {error:#}"))
}

/// The one screen: as large as a view, at 2x, with the menu bar along its top and the Dock along
/// its left side.
pub fn screen() -> Screen {
    Screen {
        id: SCREEN,
        name: "Test Display".into(),
        size: PixelSize {
            width: VIEW_WIDTH,
            height: VIEW_HEIGHT,
        },
        scale: 2.0,
        refresh_rate: 60_000,
        frame: Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: f64::from(VIEW_WIDTH) / 2.0,
                height: f64::from(VIEW_HEIGHT) / 2.0,
            },
        },
        visible_frame: Rect {
            origin: Point { x: 64.0, y: 24.0 },
            size: Size {
                width: f64::from(VIEW_WIDTH) / 2.0 - 64.0,
                height: f64::from(VIEW_HEIGHT) / 2.0 - 24.0,
            },
        },
    }
}

pub fn metrics(width: u32, height: u32) -> Metrics {
    let none = Insets {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };
    Metrics {
        width,
        height,
        device_pixel_ratio: 2.0,
        padding: none,
        view_padding: none,
        view_insets: none,
    }
}

/// A mouse's `change` at `(x, y)` in a view's physical pixels, at `time` on the program's clock,
/// with `buttons` held.
pub fn mouse(
    change: PointerChange,
    buttons: PointerButtons,
    (x, y): (f64, f64),
    time: u64,
) -> PointerEvent {
    PointerEvent {
        time,
        change,
        kind: PointerDeviceKind::Mouse,
        signal: PointerSignalKind::None,
        device: 1,
        x,
        y,
        buttons,
        pressure: 0.0,
        pressure_min: 0.0,
        pressure_max: 1.0,
        distance: 0.0,
        distance_max: 0.0,
        size: 0.0,
        radius_major: 0.0,
        radius_minor: 0.0,
        radius_min: 0.0,
        radius_max: 0.0,
        orientation: 0.0,
        tilt: 0.0,
        scroll_delta_x: 0.0,
        scroll_delta_y: 0.0,
        pan_x: 0.0,
        pan_y: 0.0,
        scale: 1.0,
        rotation: 0.0,
    }
}

pub fn frame() -> Rect {
    Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: Size {
            width: f64::from(VIEW_WIDTH) / 2.0,
            height: f64::from(VIEW_HEIGHT) / 2.0,
        },
    }
}

/// The shell: answers every request with a view, at once, refreshes its screen a moment after it
/// is asked, and keeps what it was asked.
pub struct TestShell {
    /// The engine its screen's refreshes are reported to, once it is made.
    engine: OnceLock<Weak<WaOS>>,
    /// How many of the screen's refreshes were asked for.
    refreshes_asked: AtomicUsize,
    /// Whether a test stopped the screen's refreshes.
    refreshes_stopped: AtomicBool,
    /// When each refresh's picture was expected on screen.
    display_times: Mutex<Vec<Instant>>,
    /// Where the refreshes asked for go: to the screen's thread, which ends with the shell.
    screen: Sender<ScreenId>,
    arrivals: Sender<ProgramView>,
    /// How each window asked for was to be made, in order.
    requests: Mutex<Vec<WindowConfig>>,
    /// Every view it placed, and every window.
    views: Mutex<Vec<Arc<RecordingView>>>,
    windows: Mutex<Vec<Arc<RecordingWindow>>>,
    /// Each question a program put to its user, which waits for the test's answer.
    questions: Mutex<Vec<PutQuestion>>,
    /// Each link scheme a program asked its user to open, which waits for the test's answer.
    link_schemes_asked: Mutex<Vec<PutLinkScheme>>,
    /// Each role a program asked its user for, which waits for the test's answer.
    roles_asked: Mutex<Vec<PutRole>>,
    /// Each settings page a program opened, which stays open until the test leaves it.
    settings_opened: Mutex<Vec<OpenedSettings>>,
    compositor: Arc<TestCompositor>,
    clipboard: Arc<TestClipboard>,
    menus: Arc<TestMenus>,
    sound: Arc<TestSound>,
    file_dialogs: Arc<TestFileDialogs>,
    opener: Arc<TestOpener>,
    notifications: Arc<TestNotifications>,
    media_players: Arc<TestMediaPlayers>,
    media_assets: Arc<TestMediaAssets>,
    image_codecs: Arc<TestImageCodecs>,
    video_frame_readers: Arc<TestVideoFrameReaders>,
    cameras: Arc<TestCameras>,
    /// The cameras of WaOS's own, one test pattern.
    simulated_cameras: Arc<TestCameras>,
    machine_picker: Arc<TestMachinePicker>,
}

/// A question a program put to its user, and where the answer goes until it is given.
struct PutQuestion {
    program: String,
    question: Question,
    answer: Option<oneshot::Sender<Option<Answer>>>,
}

/// A link scheme a program asked its user to open, and where the answer goes until it is given.
struct PutLinkScheme {
    program: String,
    scheme: String,
    answer: Option<oneshot::Sender<Option<Answer>>>,
}

/// A role a program asked its user for, and where the answer goes until it is given.
struct PutRole {
    program: String,
    role: Role,
    answer: Option<oneshot::Sender<Option<Answer>>>,
}

/// A settings page a program opened, and what tells it the user left, until the user does.
struct OpenedSettings {
    program: String,
    switch: Switch,
    leave: Option<oneshot::Sender<()>>,
}

impl TestShell {
    fn new() -> (Arc<TestShell>, Receiver<ProgramView>) {
        let (arrivals, arrived) = mpsc::channel();
        let (screen, asked) = mpsc::channel();
        let shell = Arc::new(TestShell {
            engine: OnceLock::new(),
            refreshes_asked: AtomicUsize::new(0),
            refreshes_stopped: AtomicBool::new(false),
            display_times: Mutex::default(),
            screen,
            arrivals,
            requests: Mutex::default(),
            views: Mutex::default(),
            windows: Mutex::default(),
            questions: Mutex::default(),
            link_schemes_asked: Mutex::default(),
            roles_asked: Mutex::default(),
            settings_opened: Mutex::default(),
            compositor: Arc::default(),
            clipboard: Arc::default(),
            menus: Arc::default(),
            sound: Arc::default(),
            file_dialogs: Arc::default(),
            opener: Arc::default(),
            notifications: Arc::default(),
            media_players: Arc::default(),
            media_assets: Arc::default(),
            image_codecs: Arc::default(),
            video_frame_readers: Arc::default(),
            cameras: Arc::default(),
            simulated_cameras: Arc::new(TestCameras::simulated()),
            machine_picker: Arc::default(),
        });
        run_screen(Arc::downgrade(&shell), asked);
        (shell, arrived)
    }

    /// The system's compositor, with the layers programs made on it.
    pub fn compositor(&self) -> &TestCompositor {
        &self.compositor
    }

    /// The clipboard, as the platform holds it.
    pub fn clipboard(&self) -> &TestClipboard {
        &self.clipboard
    }

    /// The menus, as the platform holds them.
    pub fn menus(&self) -> &TestMenus {
        &self.menus
    }

    /// The picker of a sound's output and the indicator of who records, and with what.
    pub fn sound(&self) -> &TestSound {
        &self.sound
    }

    /// The file dialogs, as the platform answers them.
    pub fn file_dialogs(&self) -> &TestFileDialogs {
        &self.file_dialogs
    }

    /// The system's opener, as the platform answers it.
    pub fn opener(&self) -> &TestOpener {
        &self.opener
    }

    /// The system's notifications, as the platform keeps them.
    pub fn notifications(&self) -> &TestNotifications {
        &self.notifications
    }

    /// The system's players, with each one opened.
    pub fn media_players(&self) -> &TestMediaPlayers {
        &self.media_players
    }

    /// The system's reading of media, with each asset opened.
    pub fn media_assets(&self) -> &TestMediaAssets {
        &self.media_assets
    }

    /// The system's image codecs, with what was asked of them.
    pub fn image_codecs(&self) -> &TestImageCodecs {
        &self.image_codecs
    }

    /// The system's frame readers, with each one made.
    pub fn video_frame_readers(&self) -> &TestVideoFrameReaders {
        &self.video_frame_readers
    }

    /// The system's cameras, with each one opened and each session started.
    pub fn cameras(&self) -> &TestCameras {
        &self.cameras
    }

    pub fn simulated_cameras(&self) -> &TestCameras {
        &self.simulated_cameras
    }

    /// The picker of a machine, which picks what the test says.
    pub fn machine_picker(&self) -> &TestMachinePicker {
        &self.machine_picker
    }

    /// How many of the screen's refreshes the engine asked for.
    pub fn refreshes_asked(&self) -> usize {
        self.refreshes_asked.load(Ordering::SeqCst)
    }

    /// Lets no more of the screen's refreshes come, as a screen that stops ticking.
    pub fn stop_refreshes(&self) {
        self.refreshes_stopped.store(true, Ordering::SeqCst);
    }

    /// The screen refreshes now, its picture expected on screen a refresh later, as the engine
    /// hears it, unless a test stopped its refreshes. The display time is kept.
    fn refresh(&self, screen: ScreenId) {
        if self.refreshes_stopped.load(Ordering::SeqCst) {
            return;
        }
        if let Some(engine) = self.engine.get().and_then(Weak::upgrade) {
            let now = Instant::now();
            let display_time = now + REFRESH_INTERVAL;
            self.display_times.lock().unwrap().push(display_time);
            engine.screen_refreshed(screen, now, display_time);
        }
    }

    /// When each refresh's picture was expected on screen, as the engine was told, in order.
    pub fn display_times(&self) -> Vec<Instant> {
        self.display_times.lock().unwrap().clone()
    }

    /// Each question put to the user so far, with the program that put it, in order.
    pub fn questions(&self) -> Vec<(String, Question)> {
        let questions = self.questions.lock().unwrap();
        questions
            .iter()
            .map(|put| (put.program.clone(), put.question))
            .collect()
    }

    /// The user answers every question waiting with `answer`.
    pub fn answer_questions(&self, answer: Option<Answer>) {
        for put in self.questions.lock().unwrap().iter_mut() {
            if let Some(waiting) = put.answer.take() {
                let _ = waiting.send(answer);
            }
        }
    }

    /// Each link scheme a program asked its user to open so far, with the program, in order.
    pub fn link_schemes_asked(&self) -> Vec<(String, String)> {
        let asked = self.link_schemes_asked.lock().unwrap();
        asked
            .iter()
            .map(|put| (put.program.clone(), put.scheme.clone()))
            .collect()
    }

    /// The user answers every link scheme waiting with `answer`.
    pub fn answer_link_schemes(&self, answer: Option<Answer>) {
        for put in self.link_schemes_asked.lock().unwrap().iter_mut() {
            if let Some(waiting) = put.answer.take() {
                let _ = waiting.send(answer);
            }
        }
    }

    /// Each role a program asked its user for so far, with the program, in order.
    pub fn roles_asked(&self) -> Vec<(String, Role)> {
        let asked = self.roles_asked.lock().unwrap();
        asked
            .iter()
            .map(|put| (put.program.clone(), put.role))
            .collect()
    }

    /// The user answers every role waiting with `answer`.
    pub fn answer_roles(&self, answer: Option<Answer>) {
        for put in self.roles_asked.lock().unwrap().iter_mut() {
            if let Some(waiting) = put.answer.take() {
                let _ = waiting.send(answer);
            }
        }
    }

    /// Each settings page opened so far, with the program that opened it, in order.
    pub fn settings_opened(&self) -> Vec<(String, Switch)> {
        let opened = self.settings_opened.lock().unwrap();
        opened
            .iter()
            .map(|opened| (opened.program.clone(), opened.switch))
            .collect()
    }

    /// The user leaves every settings page open.
    pub fn leave_settings(&self) {
        for opened in self.settings_opened.lock().unwrap().iter_mut() {
            if let Some(leave) = opened.leave.take() {
                let _ = leave.send(());
            }
        }
    }

    /// The title of each window asked for, in order.
    pub fn requested_titles(&self) -> Vec<String> {
        let requests = self.requests.lock().unwrap();
        requests.iter().map(|config| config.title.clone()).collect()
    }

    /// How each window asked for was to be made, in order.
    pub fn requested_windows(&self) -> Vec<WindowConfig> {
        self.requests.lock().unwrap().clone()
    }

    /// The shell's side of every view placed so far, in order.
    pub fn placed_views(&self) -> Vec<Arc<RecordingView>> {
        self.views.lock().unwrap().clone()
    }

    /// The shell's side of every window placed so far, in order.
    pub fn placed_windows(&self) -> Vec<Arc<RecordingWindow>> {
        self.windows.lock().unwrap().clone()
    }

    /// A view as the shell places it: focused, on the one screen, in a window; for `request`
    /// when it answers one.
    pub fn placed_view(&self, request: Option<RequestId>) -> PlacedView {
        let view = Arc::new(RecordingView::default());
        self.views.lock().unwrap().push(Arc::clone(&view));
        let window = Arc::new(RecordingWindow::default());
        self.windows.lock().unwrap().push(Arc::clone(&window));
        let text_input = Arc::clone(&view.text_input);
        PlacedView {
            view: view as Arc<dyn ShellView>,
            text_input: Some(text_input as Arc<dyn ShellTextInput>),
            window: Some(PlacedWindow {
                window: window as Arc<dyn ShellWindow>,
                frame: frame(),
                maximized: false,
                title_bar_double_click_action: Some(TitleBarAction::Maximize),
            }),
            metrics: metrics(VIEW_WIDTH, VIEW_HEIGHT),
            screen: Some(SCREEN),
            presence: Presence {
                on_screen: true,
                occluded: false,
                focused: true,
            },
            request,
        }
    }
}

impl Prompts for TestShell {
    fn ask(&self, program: &RunningProgram, question: Question) -> AnswerFuture {
        let (answer, answered) = oneshot::channel();
        self.questions.lock().unwrap().push(PutQuestion {
            program: program.name().to_owned(),
            question,
            answer: Some(answer),
        });
        Box::pin(answered.map(|answer| answer.unwrap_or(None)))
    }

    fn ask_to_open_links(&self, program: &RunningProgram, scheme: &str) -> AnswerFuture {
        let (answer, answered) = oneshot::channel();
        self.link_schemes_asked.lock().unwrap().push(PutLinkScheme {
            program: program.name().to_owned(),
            scheme: scheme.to_owned(),
            answer: Some(answer),
        });
        Box::pin(answered.map(|answer| answer.unwrap_or(None)))
    }

    fn ask_for_role(&self, program: &RunningProgram, role: Role) -> AnswerFuture {
        let (answer, answered) = oneshot::channel();
        self.roles_asked.lock().unwrap().push(PutRole {
            program: program.name().to_owned(),
            role,
            answer: Some(answer),
        });
        Box::pin(answered.map(|answer| answer.unwrap_or(None)))
    }

    fn open_settings(&self, program: &RunningProgram, switch: Switch) -> SettingsClosedFuture {
        let (leave, left) = oneshot::channel();
        self.settings_opened.lock().unwrap().push(OpenedSettings {
            program: program.name().to_owned(),
            switch,
            leave: Some(leave),
        });
        Box::pin(left.map(|_left| ()))
    }
}

impl VsyncWaiter for TestShell {
    /// Refreshes [`REFRESH_INTERVAL`] later, on the screen's thread.
    fn next_vsync(&self, screen: ScreenId) {
        self.refreshes_asked.fetch_add(1, Ordering::SeqCst);
        let _ = self.screen.send(screen);
    }
}

/// The screen's thread: each refresh asked for comes [`REFRESH_INTERVAL`] after it was asked. It
/// ends with the shell.
fn run_screen(shell: Weak<TestShell>, asked: Receiver<ScreenId>) {
    thread::Builder::new()
        .name("test screen".into())
        .spawn(move || {
            for screen in asked {
                thread::sleep(REFRESH_INTERVAL);
                match shell.upgrade() {
                    Some(shell) => shell.refresh(screen),
                    None => return,
                }
            }
        })
        .expect("a thread for the test screen");
}

impl ViewPlacement for TestShell {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        self.requests.lock().unwrap().push(request.config.clone());
        let view = program.add_view(self.placed_view(Some(request.id)));
        // A test that does not wait for arrivals has dropped the receiver.
        let _ = self.arrivals.send(view);
        Ok(())
    }
}

/// What the engine hands the shell with a hardware buffer, to call once the compositor is done with
/// it.
type Released = Box<dyn FnOnce() + Send + Sync>;

/// The shell's side of a view: records what the program asked, and shows what it presents as a
/// compositor would, holding the hardware buffer on screen until the next replaces it.
#[derive(Default)]
pub struct RecordingView {
    cursors: Mutex<Vec<Cursor>>,
    focus_changes: Mutex<Vec<(ViewFocusState, ViewFocusDirection)>>,
    /// Each time the shell was told whether the program handles Back, in order.
    handles_back: Mutex<Vec<bool>>,
    presents: AtomicUsize,
    /// The layers each present named, in the order of the presents.
    presented_layers: Mutex<Vec<Vec<PresentedLayer>>>,
    /// Whether every hardware buffer presented is kept, as a compositor that never lets one go
    /// would.
    holds_every_hardware_buffer: AtomicBool,
    /// The hardware buffers presented and not released, the oldest first: the one presented last
    /// alone, unless every one is held.
    held: Mutex<Vec<(HeldHardwareBuffer, Released)>>,
    /// Whether a present waits, and what wakes it once presents go through again.
    presents_held: Mutex<bool>,
    presents_let_through: Condvar,
    /// The view's text input, which records what the program edits.
    text_input: Arc<RecordingTextInput>,
}

/// Takes `view` off screen, as the user minimizing its window does: its presence is reported off
/// screen, and the engine serves it no frame until it shows.
pub fn hide(view: &ProgramView) {
    view.presence_changed(Presence {
        on_screen: false,
        occluded: false,
        focused: false,
    });
}

/// Covers `view` all over with other windows: its presence is reported on screen and occluded,
/// and the engine serves it no frame until it is uncovered, unless a tool watches its program.
pub fn cover(view: &ProgramView) {
    view.presence_changed(Presence {
        on_screen: true,
        occluded: true,
        focused: false,
    });
}

/// Shows `view` again: its presence is reported on screen, and a frame it asked for meanwhile
/// comes at the next refresh.
pub fn show(view: &ProgramView) {
    view.presence_changed(Presence {
        on_screen: true,
        occluded: false,
        focused: false,
    });
}

impl RecordingView {
    pub fn cursors(&self) -> Vec<Cursor> {
        self.cursors.lock().unwrap().clone()
    }

    /// Each time the shell was told whether the program handles Back, in order.
    pub fn handles_back(&self) -> Vec<bool> {
        self.handles_back.lock().unwrap().clone()
    }

    /// The kind of text each editing the program started took, in order: a text field the focus
    /// moved to starts one.
    pub fn text_inputs_started(&self) -> Vec<TextInputType> {
        self.text_input.started.lock().unwrap().clone()
    }

    /// How many frames the engine has presented to this view.
    pub fn presents(&self) -> usize {
        self.presents.load(Ordering::SeqCst)
    }

    /// The layers each present named, the bottom-most first, in the order of the presents.
    pub fn presented_layers(&self) -> Vec<Vec<PresentedLayer>> {
        self.presented_layers.lock().unwrap().clone()
    }

    /// Waits until `count` frames have been presented.
    pub fn wait_for_presents(&self, count: usize) {
        let deadline = Instant::now() + WITHIN;
        while self.presents() < count {
            assert!(
                Instant::now() < deadline,
                "{count} frames were never presented; {} were",
                self.presents()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Holds every present back from now on, the frame in it still in flight to the engine, until
    /// [`let_presents_through`](Self::let_presents_through).
    pub fn hold_presents(&self) {
        *self.presents_held.lock().unwrap() = true;
    }

    /// Lets the presents held back through, and every one after.
    pub fn let_presents_through(&self) {
        *self.presents_held.lock().unwrap() = false;
        self.presents_let_through.notify_all();
    }

    /// Waits while presents are held back.
    fn wait_while_presents_held(&self) {
        let held = self.presents_held.lock().unwrap();
        let _through = self
            .presents_let_through
            .wait_while(held, |held| *held)
            .unwrap();
    }

    /// Keeps every hardware buffer presented from now on, until a test releases one.
    pub fn hold_every_hardware_buffer(&self) {
        self.holds_every_hardware_buffer
            .store(true, Ordering::SeqCst);
    }

    /// How many hardware buffers the view holds now.
    pub fn hardware_buffers_held(&self) -> usize {
        self.held.lock().unwrap().len()
    }

    /// The hardware buffer presented last, which the view holds until the next replaces it.
    pub fn latest_hardware_buffer(&self) -> Option<HeldHardwareBuffer> {
        self.held
            .lock()
            .unwrap()
            .last()
            .map(|(buffer, _)| buffer.clone())
    }

    /// Releases every hardware buffer held but the one presented last, as a compositor does once it
    /// has shown the last.
    pub fn release_all_but_latest(&self) {
        let released: Vec<Released> = {
            let mut held = self.held.lock().unwrap();
            let last = held.len().saturating_sub(1);
            held.drain(..last).map(|(_, released)| released).collect()
        };
        for released in released {
            released();
        }
    }
}

impl ShellView for RecordingView {
    /// Shows `buffer` in place of the one before, which is released, unless every hardware buffer
    /// is held, with `layers` beneath it, which it records; once presents go through, while a test
    /// holds them back.
    fn present(
        &self,
        buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Released,
    ) {
        self.wait_while_presents_held();
        self.presented_layers
            .lock()
            .unwrap()
            .push(composition::presented(layers));
        let replaced = {
            let mut held = self.held.lock().unwrap();
            held.push((HeldHardwareBuffer(buffer), released));
            if self.holds_every_hardware_buffer.load(Ordering::SeqCst) {
                Vec::new()
            } else {
                let last = held.len() - 1;
                held.drain(..last).collect::<Vec<_>>()
            }
        };
        self.presents.fetch_add(1, Ordering::SeqCst);
        for (_, released) in replaced {
            released();
        }
    }

    fn set_cursor(&self, cursor: Cursor) {
        self.cursors.lock().unwrap().push(cursor);
    }

    fn request_focus_change(&self, state: ViewFocusState, direction: ViewFocusDirection) {
        self.focus_changes.lock().unwrap().push((state, direction));
    }

    fn set_handles_back(&self, handles: bool) {
        self.handles_back.lock().unwrap().push(handles);
    }

    fn set_overlay_style(&self, _style: OverlayStyle) {}
}

/// A text input that takes everything and shows nothing, and records the kind of text of each
/// editing started.
#[derive(Default)]
pub struct RecordingTextInput {
    started: Mutex<Vec<TextInputType>>,
}

impl ShellTextInput for RecordingTextInput {
    fn start(&self, configuration: TextInputConfiguration) {
        self.started.lock().unwrap().push(configuration.input_type);
    }

    fn set_editing_state(&self, _value: TextEditingValue) {}

    fn set_caret_rect(&self, _rect: PixelRect) {}

    fn set_composing_rect(&self, _rect: PixelRect) {}

    fn show(&self) {}

    fn hide(&self) {}

    fn stop(&self) {}
}

/// The shell's side of a window: records what the program asked, the size limits, its moves,
/// maximizing and minimizing among it, and takes its view away when the program closes it, once it
/// is told which view that is. It reports nothing of its own: a test reports the window maximized
/// or minimized itself, as a platform would.
#[derive(Default)]
pub struct RecordingWindow {
    titles: Mutex<Vec<String>>,
    closes: AtomicUsize,
    drags: AtomicUsize,
    /// Each maximizing, or return to the size from before, the program asked for, in order.
    maximized_asked: Mutex<Vec<bool>>,
    /// Each minimizing, or restoring, the program asked for, in order.
    minimized_asked: Mutex<Vec<bool>>,
    /// The smallest and the largest size the program last set, as width and height.
    min_size: Mutex<Option<(f64, f64)>>,
    max_size: Mutex<Option<(f64, f64)>>,
    /// The program and the view the window holds, once placed.
    holds: OnceLock<(RunningProgram, ViewId)>,
}

impl RecordingWindow {
    /// Each title the program gave the window after it was placed, in order.
    pub fn titles(&self) -> Vec<String> {
        self.titles.lock().unwrap().clone()
    }

    pub fn closes(&self) -> usize {
        self.closes.load(Ordering::SeqCst)
    }

    /// How many moves of the window from a press the program started.
    pub fn drags(&self) -> usize {
        self.drags.load(Ordering::SeqCst)
    }

    /// Each maximizing, or return to the size from before, the program asked for, in order.
    pub fn maximized_asked(&self) -> Vec<bool> {
        self.maximized_asked.lock().unwrap().clone()
    }

    /// Each minimizing, or restoring, the program asked for, in order.
    pub fn minimized_asked(&self) -> Vec<bool> {
        self.minimized_asked.lock().unwrap().clone()
    }

    /// The smallest size the program last set, as width and height; none until it sets one.
    pub fn min_size(&self) -> Option<(f64, f64)> {
        *self.min_size.lock().unwrap()
    }

    /// The largest size the program last set, as width and height; none until it sets one.
    pub fn max_size(&self) -> Option<(f64, f64)> {
        *self.max_size.lock().unwrap()
    }
}

impl ShellWindow for RecordingWindow {
    fn set_frame(&self, _frame: Rect, _animate: Option<Duration>) {}

    fn set_title(&self, title: &str) {
        self.titles.lock().unwrap().push(title.to_owned());
    }

    fn show(&self) {}

    fn hide(&self) {}

    fn close(&self) {
        self.closes.fetch_add(1, Ordering::SeqCst);
        if let Some((program, view)) = self.holds.get() {
            program.remove_view(*view);
        }
    }

    fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits> {
        Some(self)
    }

    fn drag(&self) {
        self.drags.fetch_add(1, Ordering::SeqCst);
    }

    fn set_maximized(&self, maximized: bool) {
        self.maximized_asked.lock().unwrap().push(maximized);
    }

    fn set_minimized(&self, minimized: bool) {
        self.minimized_asked.lock().unwrap().push(minimized);
    }
}

impl ShellWindowSizeLimits for RecordingWindow {
    fn set_min_size(&self, size: Option<Size>) {
        *self.min_size.lock().unwrap() = size.map(|size| (size.width, size.height));
    }

    fn set_max_size(&self, size: Option<Size>) {
        *self.max_size.lock().unwrap() = size.map(|size| (size.width, size.height));
    }
}

/// Waits until `count` frames have reached `shell_view`, failing with how `program` ended, and
/// what `printed` says it printed, if it ends first.
pub fn wait_for_presents(
    program: &RunningProgram,
    shell_view: &RecordingView,
    count: usize,
    printed: impl Fn() -> Vec<String>,
) {
    let deadline = Instant::now() + WITHIN;
    let mut ended = pin!(program.ended());
    while shell_view.presents() < count {
        if let Some(end) = ended.as_mut().now_or_never() {
            panic!(
                "{} ended: {end:?}; it printed {:#?}",
                program.name(),
                printed()
            );
        }
        assert!(
            Instant::now() < deadline,
            "{count} frames were never presented; {} were",
            shell_view.presents()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// What a program wrote to its standard output, line by line: an output of a test's own, which
/// runs alongside others.
#[derive(Default)]
pub struct Lines {
    bytes: Mutex<Vec<u8>>,
}

impl Lines {
    pub fn all(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.bytes.lock().unwrap())
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Whether the program wrote `line`.
    pub fn has(&self, line: &str) -> bool {
        self.count(line) > 0
    }

    /// How many times the program wrote `line`.
    pub fn count(&self, line: &str) -> usize {
        self.all().iter().filter(|written| *written == line).count()
    }

    /// Waits until the program wrote `line`, failing after [`WITHIN`] with what it wrote.
    pub fn wait_for(&self, line: &str) {
        let deadline = Instant::now() + WITHIN;
        while !self.has(line) {
            assert!(
                Instant::now() < deadline,
                "no {line:?} in {:#?}",
                self.all()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl ProgramOutput for Lines {
    fn write(&self, stream: OutputStream, bytes: &[u8]) {
        if stream == OutputStream::Stdout {
            self.bytes.lock().unwrap().extend_from_slice(bytes);
        }
    }
}

/// Waits until `done`, failing after [`WITHIN`] with what was awaited.
pub fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WITHIN;
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// `view`'s screenshot (`ProgramView::picture`), once it shows a picture, failing after
/// [`WITHIN`].
pub fn screenshot_of(view: &ProgramView) -> PixelBuffer {
    let deadline = Instant::now() + WITHIN;
    loop {
        if let Some(screenshot) = view.picture().expect("the screenshot is drawn") {
            return screenshot;
        }
        assert!(Instant::now() < deadline, "the view never showed a picture");
        thread::sleep(Duration::from_millis(5));
    }
}

/// The pixel of `picture` at (`x`, `y`), as its bytes lie: blue, green, red, alpha.
pub fn pixel_of(picture: &PixelBuffer, x: usize, y: usize) -> [u8; 4] {
    let at = y * picture.row_bytes as usize + x * 4;
    picture.bytes[at..at + 4].try_into().unwrap()
}

/// What `future` resolves to, failing after [`WITHIN`].
pub fn resolved<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> T {
    let (resolved, came) = mpsc::channel();
    thread::spawn(move || {
        // A test that timed out has dropped the receiver.
        let _ = resolved.send(futures::executor::block_on(future));
    });
    came.recv_timeout(WITHIN).expect("it resolves in time")
}

/// How `program` ended, once it has.
pub fn wait_for_end(program: &RunningProgram) -> ProgramEnd {
    let deadline = Instant::now() + WITHIN;
    let mut ended = pin!(program.ended());
    loop {
        if let Some(end) = ended.as_mut().now_or_never() {
            return end;
        }
        assert!(Instant::now() < deadline, "{} never ended", program.name());
        thread::sleep(Duration::from_millis(10));
    }
}

/// Tells the window placed with `view` which view it holds, so closing it takes the view away.
pub fn window_holds(window: &Arc<RecordingWindow>, program: &RunningProgram, view: ViewId) {
    window
        .holds
        .set((program.clone(), view))
        .unwrap_or_else(|_| panic!("a window holds one view"));
}

/// What every program printed, as the engine records it: each line with its program's name.
#[derive(Default)]
pub struct Output {
    lines: Mutex<Vec<(String, String)>>,
}

/// The glyph calls every program made, as the engine records them: how many it drew a shape for.
#[derive(Default)]
pub struct GlyphCalls {
    drawn: AtomicUsize,
}

/// The frames the engine handed over, as it records them.
#[derive(Default)]
pub struct HandedOver {
    frames: Mutex<Vec<HandedOverFrame>>,
}

/// One frame handed over to a view: the view, and every view the refresh served it with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HandedOverFrame {
    pub view: ViewId,
    pub views: Vec<ViewId>,
}

/// What every program printed. Installs this test binary's one subscriber the first time it is
/// asked for.
pub fn output() -> &'static Output {
    &recorder().output
}

/// The glyph calls every program made. Installs this test binary's one subscriber the first time
/// it is asked for.
pub fn glyph_calls() -> &'static GlyphCalls {
    &recorder().glyph_calls
}

/// The frames the engine handed over. Installs this test binary's one subscriber the first time
/// it is asked for.
pub fn frames_handed_over() -> &'static HandedOver {
    &recorder().frames
}

/// The one subscriber of this test binary: what its tests read of the engine's events.
#[derive(Default)]
struct Recorder {
    output: Output,
    glyph_calls: GlyphCalls,
    frames: HandedOver,
}

/// The recorder, installed as the global subscriber the first time it is asked for.
fn recorder() -> &'static Recorder {
    static RECORDER: OnceLock<Arc<Recorder>> = OnceLock::new();
    RECORDER.get_or_init(|| {
        let recorder = Arc::new(Recorder::default());
        tracing::subscriber::set_global_default(Arc::clone(&recorder))
            .expect("the recorder is the first subscriber");
        recorder
    })
}

impl Output {
    /// How many lines are recorded now: where a test starts reading from.
    pub fn mark(&self) -> usize {
        self.lines.lock().unwrap().len()
    }

    /// The lines `program` printed since `mark`.
    pub fn lines_of(&self, mark: usize, program: &str) -> Vec<String> {
        self.lines.lock().unwrap()[mark..]
            .iter()
            .filter(|(name, _)| name == program)
            .map(|(_, line)| line.clone())
            .collect()
    }

    /// Waits until `program` has printed, since `mark`, a line `wanted` accepts, and answers it.
    pub fn wait_for(&self, mark: usize, program: &str, wanted: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + WITHIN;
        loop {
            if let Some(line) = self
                .lines_of(mark, program)
                .into_iter()
                .find(|line| wanted(line))
            {
                return line;
            }
            assert!(
                Instant::now() < deadline,
                "{program} never printed the line waited for; it printed {:#?}",
                self.lines_of(mark, program)
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn record(&self, event: &Event<'_>) {
        let mut fields = OutputFields::default();
        event.record(&mut fields);
        if let (Some(program), Some(line)) = (fields.program, fields.message) {
            self.lines.lock().unwrap().push((program, line));
        }
    }
}

impl GlyphCalls {
    /// How many glyph calls the engine has drawn a shape for.
    pub fn drawn(&self) -> usize {
        self.drawn.load(Ordering::SeqCst)
    }

    /// Waits until the engine has drawn a shape for `count` glyph calls.
    pub fn wait_for_drawn(&self, count: usize) {
        let deadline = Instant::now() + WITHIN;
        while self.drawn() < count {
            assert!(
                Instant::now() < deadline,
                "{count} glyphs were never drawn; {} were",
                self.drawn()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn record(&self, event: &Event<'_>) {
        let mut drawn = Drawn::default();
        event.record(&mut drawn);
        if drawn.0 {
            self.drawn.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl HandedOver {
    /// How many frames are recorded now: where a test starts reading from.
    pub fn mark(&self) -> usize {
        self.frames.lock().unwrap().len()
    }

    /// The frames handed over since `mark`.
    pub fn since(&self, mark: usize) -> Vec<HandedOverFrame> {
        self.frames.lock().unwrap()[mark..].to_vec()
    }

    /// Waits until a frame handed over since `mark` is one `wanted` accepts, and answers it.
    pub fn wait_for(
        &self,
        mark: usize,
        wanted: impl Fn(&HandedOverFrame) -> bool,
    ) -> HandedOverFrame {
        let deadline = Instant::now() + WITHIN;
        loop {
            if let Some(frame) = self.frames.lock().unwrap()[mark..]
                .iter()
                .find(|frame| wanted(frame))
            {
                return frame.clone();
            }
            assert!(
                Instant::now() < deadline,
                "no frame handed over was the one waited for"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Records the frame a handed-over event names; other frame events name no `views`.
    fn record(&self, event: &Event<'_>) {
        let mut frame = FrameFields::default();
        event.record(&mut frame);
        if let (Some(view), Some(views)) = (frame.view, frame.views) {
            self.frames
                .lock()
                .unwrap()
                .push(HandedOverFrame { view, views });
        }
    }
}

impl Subscriber for Recorder {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let target = metadata.target();
        metadata.is_event()
            && (target == waos::targets::PROGRAM
                || target == waos::targets::GLYPHS
                || target == waos::targets::FRAMES)
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        match event.metadata().target() {
            waos::targets::PROGRAM => self.output.record(event),
            waos::targets::GLYPHS => self.glyph_calls.record(event),
            waos::targets::FRAMES => self.frames.record(event),
            _ => {}
        }
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

/// The two fields of a program's output event.
#[derive(Default)]
struct OutputFields {
    program: Option<String>,
    message: Option<String>,
}

impl Visit for OutputFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "program" => self.program = Some(format!("{value:?}")),
            "message" => self.message = Some(format!("{value:?}")),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "program" {
            self.program = Some(value.to_owned());
        }
    }
}

/// The `view` and `views` fields of a frame's event: the view, and the views its refresh served,
/// written as a list.
#[derive(Default)]
struct FrameFields {
    view: Option<ViewId>,
    views: Option<Vec<ViewId>>,
}

impl Visit for FrameFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "views" {
            let written = format!("{value:?}");
            let ids = written
                .trim_matches(|c| c == '[' || c == ']')
                .split(", ")
                .filter(|id| !id.is_empty())
                .map(|id| id.parse().expect("a view id"))
                .collect();
            self.views = Some(ids);
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "view" {
            self.view = Some(value);
        }
    }
}

/// The `drawn` field of a glyph call's event.
#[derive(Default)]
struct Drawn(bool);

impl Visit for Drawn {
    fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "drawn" {
            self.0 = value;
        }
    }
}
