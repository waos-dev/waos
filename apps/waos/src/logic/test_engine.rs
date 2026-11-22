//! An engine for the app's tests: no screens, screens that never refresh, the code cache the test
//! gives it, and the placement the test gives it, or one that makes no windows, with the prompts
//! the test gives it, or none to ask, keeping no days of what the programs do unless the test
//! gives it the log to keep them in, as the app gives its store; or over a shell the test fills
//! in further, on the time it gives; or, on Apple's systems and Linux, one whose programs' windows
//! draw (`drawing::engine`), built over `waos_headless`'s screen. A test that loads
//! `waos_test_programs`' programs where they are gives it the cache the tests share for them,
//! `waos_test_programs::CODE_CACHE`; one whose programs are copies at paths of its own, sent to the
//! dev server or installed from a package, gives it a cache in its own folder, removed with it.

use std::path::Path;
use std::sync::Arc;

use waos::bindings::wasdk::platform::system::Brightness;
use waos::bindings::wasdk::platform::views::ViewError;
use waos::{
    AuditLog, EngineTime, FreezePolicy, HardwareBufferMode, HttpProxy, NoPrompts, PlatformState,
    Prompts, RunningProgram, Shell, ViewPlacement, ViewRequest, VsyncWaiter, WaOS, WaOSConfig,
};

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

/// An engine placing views with `views`, with nobody to ask, keeping compiled programs in
/// `code_cache`.
pub(crate) fn engine(views: Arc<dyn ViewPlacement>, code_cache: &Path) -> WaOS {
    engine_asking(views, Arc::new(NoPrompts), code_cache)
}

/// An engine placing views with `views` and asking the user through `prompts`, keeping compiled
/// programs in `code_cache`.
pub(crate) fn engine_asking(
    views: Arc<dyn ViewPlacement>,
    prompts: Arc<dyn Prompts>,
    code_cache: &Path,
) -> WaOS {
    engine_over(shell(views, prompts), EngineTime::Real, code_cache)
}

/// An engine placing views with `views` and asking the user through `prompts`, keeping the days
/// of what its programs do in `audit` and compiled programs in `code_cache`.
pub(crate) fn engine_keeping_days(
    views: Arc<dyn ViewPlacement>,
    prompts: Arc<dyn Prompts>,
    audit: Arc<dyn AuditLog>,
    code_cache: &Path,
) -> WaOS {
    let config = WaOSConfig {
        audit: Some(audit),
        ..config(EngineTime::Real, code_cache)
    };
    engine_of(config, shell(views, prompts))
}

/// The shell of an engine for the app's tests: `views` places its views and `prompts` asks its
/// user, its screens never refresh, and it has nothing else.
pub(crate) fn shell(views: Arc<dyn ViewPlacement>, prompts: Arc<dyn Prompts>) -> Shell {
    Shell {
        views,
        vsync: Arc::new(NoRefreshes) as Arc<dyn VsyncWaiter>,
        compositor: None,
        prompts,
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
        cameras: None,
        simulated_cameras: None,
        machine_picker: None,
    }
}

/// An engine over `shell`, on `time`, with no screens, keeping compiled programs in `code_cache`.
pub(crate) fn engine_over(shell: Shell, time: EngineTime, code_cache: &Path) -> WaOS {
    engine_of(config(time, code_cache), shell)
}

/// A test engine's configuration: on `time`, keeping compiled programs in `code_cache`, and
/// nothing else.
fn config(time: EngineTime, code_cache: &Path) -> WaOSConfig {
    WaOSConfig {
        code_cache: code_cache.to_owned(),
        freeze: FreezePolicy::default(),
        native_unwind_info: false,
        http_proxy: HttpProxy::None,
        time,
        temporary_folders: std::env::temp_dir().join("waos-tests"),
        audit: None,
        inspected_programs: None,
        hardware_buffers: HardwareBufferMode::Auto,
    }
}

/// An engine of `config` over `shell`, with no screens.
fn engine_of(config: WaOSConfig, shell: Shell) -> WaOS {
    let platform = PlatformState {
        screens: Vec::new(),
        platform_brightness: Brightness::Light,
        caption_style: None,
        locales: Vec::new(),
        time_zone: None,
    };
    WaOS::new(config, shell, platform).expect("an engine")
}

/// An engine whose programs' windows draw, where the engine makes hardware buffers (or Linux
/// offscreen buffers).
#[cfg(any(target_vendor = "apple", target_os = "linux"))]
pub(crate) mod drawing {
    use std::path::Path;
    use std::sync::Arc;

    use waos::bindings::wasdk::platform::system::Brightness;
    use waos::bindings::wasdk::platform::views::RequestId;
    use waos::{
        EngineTime, FreezePolicy, HardwareBufferMode, HttpProxy, NoPrompts, PlatformState,
        ProgramView, RunningProgram, WaOS, WaOSConfig,
    };

    use waos_headless::{Display, HeadlessScreen, Refreshes};

    /// The size of a window [`engine`]'s shell gives, in physical pixels, at 2x.
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

    /// An engine whose programs' windows draw: each request given a window at once, focused and in
    /// sight, on one screen that refreshes as soon as it is asked, each window keeping the
    /// picture presented last until the next replaces it; with nobody to ask, keeping compiled
    /// programs in `code_cache`.
    pub(crate) fn engine(code_cache: &Path) -> Arc<WaOS> {
        engine_giving(true, code_cache)
    }

    /// An engine as [`engine`]'s, but each view given in no window, as a phone's deck shows a tab.
    pub(crate) fn deck_engine(code_cache: &Path) -> Arc<WaOS> {
        engine_giving(false, code_cache)
    }

    /// An engine as [`engine`]'s, placing views in windows when `in_windows`.
    fn engine_giving(in_windows: bool, code_cache: &Path) -> Arc<WaOS> {
        let screen = HeadlessScreen::new(display(in_windows), Refreshes::WhenAsked);
        let shell = screen.shell(Arc::new(NoPrompts));
        let config = WaOSConfig {
            code_cache: code_cache.to_owned(),
            freeze: FreezePolicy::default(),
            native_unwind_info: false,
            http_proxy: HttpProxy::None,
            time: EngineTime::Real,
            temporary_folders: std::env::temp_dir().join("waos-tests"),
            audit: None,
            inspected_programs: None,
            hardware_buffers: HardwareBufferMode::Offscreen,
        };
        let platform = PlatformState {
            screens: vec![screen.screen()],
            platform_brightness: Brightness::Light,
            caption_style: None,
            locales: Vec::new(),
            time_zone: None,
        };
        Arc::new(WaOS::new(config, shell, platform).expect("an engine"))
    }

    /// Opens a view for `program`'s `request` in a window of its own, filling the test screen.
    pub(crate) fn add_window(program: &RunningProgram, request: RequestId) -> ProgramView {
        HeadlessScreen::new(display(true), Refreshes::WhenAsked).add_view(program, request)
    }
}
