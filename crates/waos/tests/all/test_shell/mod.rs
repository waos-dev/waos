//! The engine's tests' shell: the headless shell's windowing parts (`waos_headless`), and a
//! stand-in for every other part of the platform, which a test plays: the user's answers to what
//! an app's permissions ask (`prompts`), the clipboard, the menus, the dialogs, the cameras and the rest,
//! each recording what the engine asked of it. A test makes one, makes an engine over it with the
//! engine's own calls, and reads back what the engine told it; what a program prints, it reads
//! from the program's own output (`output`), and whatever else it waits for, it waits for on the
//! shell's changes (`TestShell::changes`), which every part announces, or with one deadline
//! (`within`), as `waos_headless::testing` has them.
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
use std::pin::Pin;
use std::sync::Arc;
use std::sync::mpsc;
use std::task::{Context, Poll};

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
use waos_headless::testing::Changes;
pub use waos_headless::testing::{WITHIN, poll_until, within};
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
    /// The changes to what any part was told, the headless shell's and every stand-in's: what a
    /// test waits on for what a part keeps but does not announce to the engine.
    pub changes: Arc<Changes>,
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
        let headless = HeadlessShell::new(DISPLAY, Refreshes::WhenAsked);
        let changes = Arc::clone(headless.changes());
        let notifications = Arc::new(TestNotifications::announcing(&changes));
        TestShell {
            headless,
            prompts: Arc::new(TestPrompts::announcing(&changes)),
            compositor: Arc::new(TestCompositor::announcing(&changes)),
            clipboard: Arc::new(TestClipboard::announcing(&changes)),
            menus: Arc::new(TestMenus::announcing(&changes)),
            sound: Arc::new(TestSound::announcing(&changes)),
            file_dialogs: Arc::new(TestFileDialogs::announcing(&changes)),
            opener: Arc::new(TestOpener::announcing(&changes)),
            notifications: Arc::clone(&notifications),
            system_permissions: Arc::new(TestSystemPermissions::new(notifications, &changes)),
            media_players: Arc::new(TestMediaPlayers::announcing(&changes)),
            media_assets: Arc::new(TestMediaAssets::announcing(&changes)),
            image_codecs: Arc::new(TestImageCodecs::announcing(&changes)),
            video_frame_readers: Arc::new(TestVideoFrameReaders::announcing(&changes)),
            gpus: Arc::new(TestGpus::announcing(&changes)),
            cameras: Arc::new(TestCameras::announcing(&changes)),
            simulated_cameras: Arc::new(TestCameras::simulated(&changes)),
            machine_picker: Arc::new(TestMachinePicker::announcing(&changes)),
            changes,
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
    }
}

// What the tests wait for with `poll_until` (`waos_headless::testing`), since no part announces
// it, and what would let them wait on a change instead:
// - a worker's end in `RunningProgram::stats` (`workers`): the engine announcing its workers'
//   ends, as `RunningProgram::ended` does the program's;
// - a process of this computer gone (`machines`): the local processes' capability saying when a
//   process it started has ended;
// - the engine letting go of what the shell handed it (`Arc::strong_count` in `video_decoders`
//   and `composition_layers`): the shell's part handed to the engine as a value whose drop
//   announces, as `announced_when_dropped` does for a future;
// - a file a program writes (`notes`, `video_player`): a line the program prints once it is
//   written.

/// `future`, announcing to `changes` once it is dropped, answered or not: for a stand-in's held
/// answer, so a test waits on the shell's changes for the engine to let go of it.
pub fn announced_when_dropped<T: 'static>(
    changes: &Arc<Changes>,
    future: impl Future<Output = T> + Send + 'static,
) -> Pin<Box<dyn Future<Output = T> + Send>> {
    Box::pin(AnnouncedWhenDropped {
        future: Some(Box::pin(future)),
        changes: Arc::clone(changes),
    })
}

/// A future that announces its drop once what it wraps is dropped, so a wait sees the drop.
struct AnnouncedWhenDropped<T> {
    future: Option<Pin<Box<dyn Future<Output = T> + Send>>>,
    changes: Arc<Changes>,
}

impl<T> Future for AnnouncedWhenDropped<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        self.future
            .as_mut()
            .expect("a future polled after its drop")
            .as_mut()
            .poll(cx)
    }
}

impl<T> Drop for AnnouncedWhenDropped<T> {
    fn drop(&mut self) {
        drop(self.future.take());
        self.changes.announce();
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
