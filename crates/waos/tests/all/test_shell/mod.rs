//! The engine's tests' shell: the headless shell's windowing parts (`waos_headless`), and a
//! stand-in for every other part of the platform, which a test plays: the user's answers to the
//! engine's questions (`prompts`), the clipboard, the menus, the dialogs, the cameras and the rest,
//! each recording what the engine asked of it. A test makes one, makes an engine over it with the
//! engine's own calls, and reads back what the engine told it; what a program prints, it reads
//! from the program's own output (`output`), and whatever it waits for, it waits for with one
//! deadline (`within`).
//!
//! The invariant: a test reads only its own shell and its own programs' outputs, so the tests run
//! side by side in one process without seeing each other.

mod events;
mod output;
mod prompts;

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use waos::bindings::wasdk::platform::system::{Brightness, Locale};
use waos::{
    EngineTime, FreezePolicy, HttpProxy, PlatformState, ProgramEnvironment, ProgramOutput, Prompts,
    Shell, ShellAudioOutputPicker, ShellCameras, ShellClipboard, ShellCompositor,
    ShellContextMenus, ShellFileDialogs, ShellImageCodecs, ShellMachinePicker, ShellMediaAssets,
    ShellMediaPlayers, ShellMenuBar, ShellNotifications, ShellOpener, ShellRecordingIndicator,
    ShellTrayIcons, ShellVideoFrameReaders, WaOSConfig,
};
use waos_headless::{Display, HeadlessShell, Refreshes};

use crate::shell::cameras::TestCameras;
use crate::shell::clipboard::TestClipboard;
use crate::shell::composition::TestCompositor;
use crate::shell::file_dialogs::TestFileDialogs;
use crate::shell::image_codecs::TestImageCodecs;
use crate::shell::machines::TestMachinePicker;
use crate::shell::media_assets::TestMediaAssets;
use crate::shell::media_players::TestMediaPlayers;
use crate::shell::menus::TestMenus;
use crate::shell::notifications::TestNotifications;
use crate::shell::opener::TestOpener;
use crate::shell::sound::TestSound;
use crate::shell::video_frames::TestVideoFrameReaders;
pub use events::{metrics, mouse};
pub use output::RecordedOutput;
pub use prompts::TestPrompts;

/// How long a test waits for what it expects before it fails: long enough that only a broken
/// build reaches it, a debug build of Waft starting on a busy machine among what does not.
pub const WITHIN: Duration = Duration::from_secs(60);

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
    pub media_players: Arc<TestMediaPlayers>,
    pub media_assets: Arc<TestMediaAssets>,
    pub image_codecs: Arc<TestImageCodecs>,
    pub video_frame_readers: Arc<TestVideoFrameReaders>,
    pub cameras: Arc<TestCameras>,
    pub simulated_cameras: Arc<TestCameras>,
    pub machine_picker: Arc<TestMachinePicker>,
}

impl TestShell {
    /// A shell over [`DISPLAY`], which refreshes as soon as a frame is asked for, nothing asked of
    /// any part yet.
    pub fn new() -> TestShell {
        TestShell {
            headless: HeadlessShell::new(DISPLAY, Refreshes::WhenAsked),
            prompts: Arc::default(),
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
            media_players: Some(Arc::clone(&self.media_players) as Arc<dyn ShellMediaPlayers>),
            media_assets: Some(Arc::clone(&self.media_assets) as Arc<dyn ShellMediaAssets>),
            image_codecs: Some(Arc::clone(&self.image_codecs) as Arc<dyn ShellImageCodecs>),
            video_frame_readers: Some(
                Arc::clone(&self.video_frame_readers) as Arc<dyn ShellVideoFrameReaders>
            ),
            cameras: Some(Arc::clone(&self.cameras) as Arc<dyn ShellCameras>),
            simulated_cameras: Some(Arc::clone(&self.simulated_cameras) as Arc<dyn ShellCameras>),
            machine_picker: Some(Arc::clone(&self.machine_picker) as Arc<dyn ShellMachinePicker>),
            ..self
                .headless
                .shell(Arc::clone(&self.prompts) as Arc<dyn Prompts>)
        }
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
        hardware_buffers: waos::HardwareBufferMode::Auto,
    }
}

/// What `future` resolves with, failing after [`WITHIN`]: the one wait a test has, for what it
/// expects. It blocks the test's thread; the programs run on threads of their own.
pub fn within<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> T {
    let (resolved, came) = mpsc::channel();
    thread::spawn(move || {
        // A test that timed out has dropped the receiver.
        let _ = resolved.send(futures::executor::block_on(future));
    });
    came.recv_timeout(WITHIN)
        .unwrap_or_else(|_| panic!("what the test waited for never came within {WITHIN:?}"))
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
