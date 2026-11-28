//! An engine for the app's tests, and the shells a test's WaOS starts over (`startup`): no
//! screens, screens that never refresh, and the placement the test gives it, or one that makes no
//! windows, keeping no days of what the programs do; or over a shell the test fills in further, on
//! the time it gives; or, on Apple's systems and Linux, a shell whose programs' windows draw
//! (`drawing::outside`), built over `waos_headless`'s screen. A test loads
//! `waos_test_programs`' programs from their code folders (`waos_test_programs::code_of`), and
//! installs them with their code compiled already (`test_installs`).

use std::sync::Arc;
use std::time::Duration;

use waos::bindings::wasdk::platform::system::Brightness;
use waos::bindings::wasdk::platform::views::ViewError;
use waos::{
    EngineTime, FreezePolicy, HttpProxy, PlatformState, RunningProgram, Shell, ViewPlacement,
    ViewRequest, VsyncWaiter, WaOS, WaOSConfig,
};

/// How long a test waits for a program or the app to do something before it fails. No wait
/// includes compiling a program the test did not install itself: the tests share compiled code.
pub(crate) const WITHIN: Duration = Duration::from_secs(15);

/// Why [`NoWindows`] refuses every request.
pub(crate) const REFUSAL: &str = "the tests make no windows";

/// A placement with no windows, for programs that run with none.
pub(crate) struct NoWindows;

impl ViewPlacement for NoWindows {
    fn view_requested(
        &self,
        _program: &RunningProgram,
        _request: ViewRequest,
    ) -> Result<(), ViewError> {
        Err(ViewError::Failed(REFUSAL.to_owned()))
    }
}

/// Screens that never refresh, for an engine whose programs draw nothing.
pub(crate) struct NoRefreshes;

impl VsyncWaiter for NoRefreshes {
    fn next_vsync(&self, _screen: u64, _on_refresh: waos::OnRefresh) {}
}

/// An engine placing views with `views`.
pub(crate) fn engine(views: Arc<dyn ViewPlacement>) -> WaOS {
    engine_over(shell(views), EngineTime::Real)
}

/// The shell of an engine for the app's tests: `views` places its views, its screens never
/// refresh, and it has nothing else.
pub(crate) fn shell(views: Arc<dyn ViewPlacement>) -> Shell {
    Shell {
        views,
        vsync: Arc::new(NoRefreshes) as Arc<dyn VsyncWaiter>,
        compositor: None,
        system_permissions: None,
        clipboard: None,
        menu_bar: None,
        context_menus: None,
        tray_icons: None,
        audio_output_picker: None,
        recording_indicator: None,
        file_dialogs: None,
        opener: None,
        notifications: None,
        media_players: None,
        media_assets: None,
        image_codecs: None,
        video_frame_readers: None,
        gpus: None,
        cameras: None,
        simulated_cameras: None,
        machine_picker: None,
    }
}

/// An engine over `shell`, on `time`, with no screens.
pub(crate) fn engine_over(shell: Shell, time: EngineTime) -> WaOS {
    engine_of(config(time), shell)
}

/// A test engine's configuration: on `time`, and nothing else.
fn config(time: EngineTime) -> WaOSConfig {
    WaOSConfig {
        freeze: FreezePolicy::default(),
        compiler: waos::Compiler::new(false).expect("a compiler"),
        http_proxy: HttpProxy::None,
        time,
        temporary_folders: std::env::temp_dir().join("waos-tests"),
        audit: None,
        inspected_programs: None,
    }
}

/// An engine of `config` over `shell`, with no screens.
fn engine_of(config: WaOSConfig, shell: Shell) -> WaOS {
    WaOS::new(config, shell, no_screens()).expect("an engine")
}

/// A machine with no screens, light, of no language or time zone.
pub(crate) fn no_screens() -> PlatformState {
    PlatformState {
        screens: Vec::new(),
        platform_brightness: Brightness::Light,
        caption_style: None,
        locales: Vec::new(),
        time_zone: None,
    }
}

/// A shell whose programs' windows draw, where the engine makes hardware buffers (or Linux
/// offscreen buffers).
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
pub(crate) mod drawing {
    use waos::bindings::wasdk::platform::system::Brightness;
    use waos::bindings::wasdk::platform::views::RequestId;
    use waos::{PlatformState, ProgramView, RunningProgram, ViewRequest, WindowConfig};

    use waos_headless::{Display, HeadlessShell, Refreshes};

    /// The size of a window [`outside`]'s shell gives, in physical pixels, at 2x.
    pub(crate) const WINDOW_WIDTH: u32 = 400;
    pub(crate) const WINDOW_HEIGHT: u32 = 300;

    /// The screen's display, in windows or, for a deck, in none.
    fn display(windows: bool) -> Display {
        Display {
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            scale: 2.0,
            windows,
        }
    }

    /// What a test's WaOS whose apps' windows draw runs over: each request given a window at once
    /// when `in_windows`, or a view in none, as a phone's deck shows a tab, focused and in sight, on
    /// one screen that refreshes as soon as it is asked, each window keeping the picture presented
    /// last until the next replaces it: its shell, and the machine with the one screen.
    pub(crate) fn outside(in_windows: bool) -> (waos::Shell, PlatformState) {
        let headless = HeadlessShell::new(display(in_windows), Refreshes::WhenAsked);
        let platform = PlatformState {
            screens: vec![headless.screen()],
            platform_brightness: Brightness::Light,
            caption_style: None,
            locales: Vec::new(),
            time_zone: None,
        };
        (headless.shell(), platform)
    }

    /// Opens a view for `program`'s `request` in a window of its own, filling the test screen.
    pub(crate) fn add_window(program: &RunningProgram, request: RequestId) -> ProgramView {
        let request = ViewRequest {
            id: request,
            config: WindowConfig::default(),
        };
        HeadlessShell::new(display(true), Refreshes::WhenAsked).add_view(program, request)
    }
}
