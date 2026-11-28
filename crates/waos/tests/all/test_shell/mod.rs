//! The engine's tests' shell: the headless shell's windowing parts (`waos_headless`), and a
//! stand-in for every other part of the platform, which a test plays: the user's answers to what
//! an app's permissions ask (`prompts`), the clipboard, the menus, the dialogs, the cameras and the rest,
//! each recording what the engine asked of it. A test makes one, makes an engine over it with the
//! engine's own calls, and reads back what the engine told it; what a program prints, it reads
//! from the program's own output (`output`), and whatever it waits for, it waits for with one
//! deadline (`within`).
//!
//! The invariant: a test reads only its own shell and its own programs' outputs, so the tests run
//! side by side in one process without seeing each other.

// Each test uses the part of the stand-ins it needs.
#![allow(dead_code)]

pub mod cameras;
pub mod clipboard;
pub mod composition;
mod engine_log;
mod events;
pub mod file_dialogs;
pub mod gpus;
pub mod hardware_buffer;
pub mod image_codecs;
pub mod machines;
pub mod media_assets;
pub mod media_players;
pub mod menus;
pub mod notifications;
pub mod opener;
mod output;
mod prompts;
pub mod sound;
pub mod system_permissions;
pub mod video_decoders;
pub mod video_frames;

use std::future::Future;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use waos::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
use waos::bindings::wasdk::platform::system::{Brightness, Locale};
use waos::bindings::wasdk::platform::views::ViewError;
use waos::{
    Compiler, EngineTime, FreezePolicy, HttpProxy, PlacedView, PlatformState, Presence,
    ProgramEnvironment, ProgramOutput, ProgramView, RunningProgram, Shell, ShellAudioOutputPicker,
    ShellCameras, ShellClipboard, ShellCompositor, ShellContextMenus, ShellFileDialogs, ShellGpus,
    ShellImageCodecs, ShellMachinePicker, ShellMediaAssets, ShellMediaPlayers, ShellMenuBar,
    ShellNotifications, ShellOpener, ShellRecordingIndicator, ShellSystemPermissions,
    ShellTrayIcons, ShellVideoFrameReaders, ShellView, ViewPlacement, ViewRequest, WaOSConfig,
};
use waos_headless::{Display, HeadlessShell, Refreshes, SCREEN};
use waos_services::packages::NothingInstalled;
use waos_services::permissions::{Permissions, Prompts, SettingsInMemory};

use cameras::TestCameras;
use clipboard::TestClipboard;
use composition::TestCompositor;
pub use engine_log::engine_log;
pub use events::{metrics, mouse};
use file_dialogs::TestFileDialogs;
use gpus::TestGpus;
use image_codecs::TestImageCodecs;
use machines::TestMachinePicker;
use media_assets::TestMediaAssets;
use media_players::TestMediaPlayers;
use menus::TestMenus;
use notifications::TestNotifications;
use opener::TestOpener;
pub use output::RecordedOutput;
pub use prompts::TestPrompts;
use sound::TestSound;
use system_permissions::TestSystemPermissions;
use video_frames::TestVideoFrameReaders;

/// How long a test waits for what it expects before it fails: longer than any passing test takes
/// alone, so that a hung wait costs seconds, not minutes.
pub const WITHIN: Duration = Duration::from_secs(15);

/// The screen the tests' programs are shown on: 1600 by 1000 pixels at 2x, each view in a window
/// of its own.
pub const DISPLAY: Display = Display {
    width: 1600,
    height: 1000,
    scale: 2.0,
    windows: true,
};

/// The headless shell's windowing parts and a stand-in for every other part of the platform.
pub struct TestShell {
    pub headless: HeadlessShell,
    pub prompts: Arc<TestPrompts>,
    pub compositor: Arc<TestCompositor>,
    pub clipboard: Arc<TestClipboard>,
    pub menus: Arc<TestMenus>,
    pub sound: Arc<TestSound>,
    pub file_dialogs: Arc<TestFileDialogs>,
    pub opener: Arc<TestOpener>,
    pub notifications: Arc<TestNotifications>,
    pub system_permissions: Arc<TestSystemPermissions>,
    pub media_players: Arc<TestMediaPlayers>,
    pub media_assets: Arc<TestMediaAssets>,
    pub image_codecs: Arc<TestImageCodecs>,
    pub video_frame_readers: Arc<TestVideoFrameReaders>,
    pub gpus: Arc<TestGpus>,
    pub cameras: Arc<TestCameras>,
    pub simulated_cameras: Arc<TestCameras>,
    pub machine_picker: Arc<TestMachinePicker>,
}

impl TestShell {
    /// A shell over [`DISPLAY`], which refreshes as soon as a frame is asked for, nothing asked of
    /// any part yet.
    pub fn new() -> TestShell {
        let notifications = Arc::<TestNotifications>::default();
        TestShell {
            headless: HeadlessShell::new(DISPLAY, Refreshes::WhenAsked),
            prompts: Arc::default(),
            compositor: Arc::default(),
            clipboard: Arc::default(),
            menus: Arc::default(),
            sound: Arc::default(),
            file_dialogs: Arc::default(),
            opener: Arc::default(),
            notifications: Arc::clone(&notifications),
            system_permissions: Arc::new(TestSystemPermissions::new(notifications)),
            media_players: Arc::default(),
            media_assets: Arc::default(),
            image_codecs: Arc::default(),
            video_frame_readers: Arc::default(),
            gpus: Arc::default(),
            cameras: Arc::default(),
            simulated_cameras: Arc::new(TestCameras::simulated()),
            machine_picker: Arc::default(),
        }
    }

    /// The engine's shell of these parts, every one of them there.
    pub fn shell(&self) -> Shell {
        Shell {
            compositor: Some(Arc::clone(&self.compositor) as Arc<dyn ShellCompositor>),
            clipboard: Some(Arc::clone(&self.clipboard) as Arc<dyn ShellClipboard>),
            menu_bar: Some(Arc::clone(&self.menus) as Arc<dyn ShellMenuBar>),
            context_menus: Some(Arc::clone(&self.menus) as Arc<dyn ShellContextMenus>),
            tray_icons: Some(Arc::clone(&self.menus) as Arc<dyn ShellTrayIcons>),
            audio_output_picker: Some(Arc::clone(&self.sound) as Arc<dyn ShellAudioOutputPicker>),
            recording_indicator: Some(Arc::clone(&self.sound) as Arc<dyn ShellRecordingIndicator>),
            file_dialogs: Some(Arc::clone(&self.file_dialogs) as Arc<dyn ShellFileDialogs>),
            opener: Some(Arc::clone(&self.opener) as Arc<dyn ShellOpener>),
            notifications: Some(Arc::clone(&self.notifications) as Arc<dyn ShellNotifications>),
            system_permissions: Some(
                Arc::clone(&self.system_permissions) as Arc<dyn ShellSystemPermissions>
            ),
            media_players: Some(Arc::clone(&self.media_players) as Arc<dyn ShellMediaPlayers>),
            media_assets: Some(Arc::clone(&self.media_assets) as Arc<dyn ShellMediaAssets>),
            image_codecs: Some(Arc::clone(&self.image_codecs) as Arc<dyn ShellImageCodecs>),
            video_frame_readers: Some(
                Arc::clone(&self.video_frame_readers) as Arc<dyn ShellVideoFrameReaders>
            ),
            gpus: Some(Arc::clone(&self.gpus) as Arc<dyn ShellGpus>),
            cameras: Some(Arc::clone(&self.cameras) as Arc<dyn ShellCameras>),
            simulated_cameras: Some(Arc::clone(&self.simulated_cameras) as Arc<dyn ShellCameras>),
            machine_picker: Some(Arc::clone(&self.machine_picker) as Arc<dyn ShellMachinePicker>),
            ..self.headless.shell()
        }
    }

    /// The permissions an app the test runs is handed, as WaOS hands them: the real ones, over
    /// settings in memory at WaOS's defaults, asking the user the test plays (`prompts`).
    pub fn permissions(&self) -> Permissions {
        let prompts = Arc::clone(&self.prompts) as Arc<dyn Prompts>;
        let settings = Arc::new(SettingsInMemory::default());
        Permissions::open(settings, prompts, Arc::new(NothingInstalled))
    }

    /// The platform as the engine starts with it: the shell's one screen, light, in American
    /// English, in Tokyo's time zone.
    pub fn platform(&self) -> PlatformState {
        PlatformState {
            screens: vec![self.headless.screen()],
            platform_brightness: Brightness::Light,
            caption_style: None,
            locales: vec![Locale {
                language: "en".into(),
                script: None,
                region: Some("US".into()),
            }],
            time_zone: Some("Asia/Tokyo".into()),
        }
    }
}

/// How the tests' engines are set up unless a test says otherwise: freezing by the default policy, no native unwind information, as an app has it, HTTP
/// through no proxy, whatever the machine's settings, real time, and the programs' `/tmp` in a
/// folder of the tests' own in the system's temporary folder.
pub fn config() -> WaOSConfig {
    WaOSConfig {
        freeze: FreezePolicy::default(),
        compiler: Compiler::new(false).unwrap(),
        http_proxy: HttpProxy::None,
        time: EngineTime::Real,
        temporary_folders: std::env::temp_dir().join("waos-tests"),
        audit: None,
        inspected_programs: None,
    }
}

/// What `future` resolves with, failing after [`WITHIN`] with the line that waited: the one wait a
/// test has, for what it expects. It blocks the test's thread; the programs run on threads of
/// their own.
#[track_caller]
pub fn within<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> T {
    let waiting_at = std::panic::Location::caller();
    let (resolved, came) = mpsc::channel();
    thread::spawn(move || {
        // A test that timed out has dropped the receiver.
        let _ = resolved.send(futures::executor::block_on(future));
    });
    came.recv_timeout(WITHIN).unwrap_or_else(|_| {
        panic!("what the test waited for at {waiting_at} never came within {WITHIN:?}")
    })
}

/// Waits until `done`, failing after [`WITHIN`] with `what` was awaited: for what a part of the
/// shell was told, which it keeps but does not announce.
pub fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WITHIN;
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A program's environment as the tests give it, but for what it prints, which goes to `output`.
pub fn printing_to(output: &Arc<RecordedOutput>) -> ProgramEnvironment {
    ProgramEnvironment {
        output: Some(Arc::clone(output) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    }
}

/// The pixel of `picture` at (`x`, `y`), as its bytes lie: blue, green, red, alpha, as a view's
/// picture (`ProgramView::picture`) holds them.
pub fn pixel_of(picture: &PixelBuffer, x: usize, y: usize) -> [u8; 4] {
    let at = y * picture.row_bytes as usize + x * 4;
    picture.bytes[at..at + 4].try_into().unwrap()
}

/// Places `view` for a program's request on the test shell's screen, focused and in sight, and
/// hands back its [`ProgramView`] through the returned receiver: for a test whose view holds
/// hardware buffers back rather than replacing them as [`HeadlessShell`] does.
pub fn place_view(
    view: Arc<dyn ShellView>,
) -> (Arc<dyn ViewPlacement>, mpsc::Receiver<ProgramView>) {
    let (arrivals, arrived) = mpsc::channel();
    (Arc::new(SingleView { view, arrivals }), arrived)
}

struct SingleView {
    view: Arc<dyn ShellView>,
    arrivals: mpsc::Sender<ProgramView>,
}

impl ViewPlacement for SingleView {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        let view = program.add_view(
            request.id,
            PlacedView {
                view: Arc::clone(&self.view),
                text_input: None,
                system_bars: None,
                window: None,
                metrics: metrics(DISPLAY.width, DISPLAY.height),
                screen: Some(SCREEN),
                presence: Presence {
                    on_screen: true,
                    occluded: false,
                    focused: true,
                },
            },
        );
        let _ = self.arrivals.send(view);
        Ok(())
    }
}
