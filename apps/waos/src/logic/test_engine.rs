//! An engine for the app's tests: no screens, screens that never refresh, the code cache the test
//! gives it, and the placement the test gives it, or one that makes no windows, with the prompts
//! the test gives it, or none to ask; or over a shell the test fills in further, on the time it
//! gives; or, on Apple's systems, one whose programs' windows draw (`drawing::engine`): each
//! request given a window at once, on one screen that refreshes a moment after it is asked, each
//! window keeping the picture presented last until the next replaces it, as a compositor does, and
//! taking the frame it is given, its view filling it. A
//! test that loads `waos_test_programs`' programs where they are gives it the cache the tests share
//! for them, `waos_test_programs::CODE_CACHE`; one whose programs are copies at paths of its own,
//! sent to the dev server or installed from a package, gives it a cache in its own folder, removed
//! with it.

use std::path::Path;
use std::sync::Arc;

use waos::bindings::wasdk::platform::system::Brightness;
use waos::bindings::wasdk::platform::views::ViewError;
use waos::{
    EngineTime, FreezePolicy, HttpProxy, NoPrompts, PlatformState, Prompts, RunningProgram, Shell,
    ViewPlacement, ViewRequest, VsyncWaiter, WaOS, WaOSConfig,
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
    fn next_vsync(&self, _screen: u64) {}
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
    let config = WaOSConfig {
        code_cache: code_cache.to_owned(),
        freeze: FreezePolicy::default(),
        native_unwind_info: false,
        http_proxy: HttpProxy::None,
        time,
        temporary_folders: std::env::temp_dir().join("waos-tests"),
    };
    let platform = PlatformState {
        screens: Vec::new(),
        platform_brightness: Brightness::Light,
        caption_style: None,
        locales: Vec::new(),
        time_zone: None,
    };
    WaOS::new(config, shell, platform).expect("an engine")
}

/// An engine whose programs' windows draw, where the engine makes hardware buffers.
#[cfg(target_vendor = "apple")]
pub(crate) mod drawing {
    use std::path::Path;
    use std::sync::{Arc, Mutex, OnceLock, Weak};
    use std::thread;
    use std::time::{Duration, Instant};

    use waos::bindings::wasdk::platform::screens::PixelSize;
    use waos::bindings::wasdk::platform::system::Brightness;
    use waos::bindings::wasdk::platform::views::{
        Cursor, Insets, Metrics, OverlayStyle, PixelRect, Placement, TextEditingValue,
        TextInputConfiguration, ViewError, ViewFocusDirection, ViewFocusState,
    };
    use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};
    use waos::{
        EngineTime, FreezePolicy, HardwareBuffer, HttpProxy, NoPrompts, PlacedView, PlacedWindow,
        PlatformState, Presence, ProgramView, RunningProgram, Screen, ShellCompositionLayer,
        ShellTextInput, ShellView, ShellWindow, ShellWindowSizeLimits, ViewPlacement, ViewRequest,
        VsyncWaiter, WaOS, WaOSConfig,
    };

    /// The size of a window [`engine`]'s shell gives, in physical pixels, at 2x.
    pub(crate) const WINDOW_WIDTH: u32 = 400;
    pub(crate) const WINDOW_HEIGHT: u32 = 300;

    /// The one screen of [`engine`]'s shell.
    const SCREEN: u64 = 1;

    /// How long after the engine asks the screen refreshes.
    const REFRESH_INTERVAL: Duration = Duration::from_millis(16);

    /// An engine whose programs' windows draw: each request given a window at once, focused and in
    /// sight, on one screen that refreshes [`REFRESH_INTERVAL`] after it is asked, each window
    /// keeping the picture presented last until the next replaces it; with nobody to ask, keeping
    /// compiled programs in `code_cache`.
    pub(crate) fn engine(code_cache: &Path) -> Arc<WaOS> {
        engine_giving(GivenViews::InWindows, code_cache)
    }

    /// An engine as [`engine`]'s, but each view given in no window, as a phone's deck shows a tab.
    pub(crate) fn deck_engine(code_cache: &Path) -> Arc<WaOS> {
        engine_giving(GivenViews::InNoWindow, code_cache)
    }

    /// An engine as [`engine`]'s, giving views as `views` says.
    fn engine_giving(views: GivenViews, code_cache: &Path) -> Arc<WaOS> {
        let refreshes = Arc::new(Refreshes::default());
        let mut shell = super::shell(Arc::new(views), Arc::new(NoPrompts));
        shell.vsync = Arc::clone(&refreshes) as Arc<dyn VsyncWaiter>;
        let config = WaOSConfig {
            code_cache: code_cache.to_owned(),
            freeze: FreezePolicy::default(),
            native_unwind_info: false,
            http_proxy: HttpProxy::None,
            time: EngineTime::Real,
            temporary_folders: std::env::temp_dir().join("waos-tests"),
        };
        let platform = PlatformState {
            screens: vec![screen()],
            platform_brightness: Brightness::Light,
            caption_style: None,
            locales: Vec::new(),
            time_zone: None,
        };
        let engine = Arc::new(WaOS::new(config, shell, platform).expect("an engine"));
        let _ = refreshes.engine.set(Arc::downgrade(&engine));
        engine
    }

    /// The screen windows are on: as large as one, at 2x.
    fn screen() -> Screen {
        let points = Size {
            width: f64::from(WINDOW_WIDTH) / 2.0,
            height: f64::from(WINDOW_HEIGHT) / 2.0,
        };
        let frame = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: points,
        };
        Screen {
            id: SCREEN,
            name: "Test Display".into(),
            size: PixelSize {
                width: WINDOW_WIDTH,
                height: WINDOW_HEIGHT,
            },
            scale: 2.0,
            refresh_rate: 60_000,
            frame,
            visible_frame: frame,
        }
    }

    /// Each request given a view at once, filling the screen, focused and in sight: in a window
    /// that takes the frame it is given, or in none.
    enum GivenViews {
        InWindows,
        InNoWindow,
    }

    impl ViewPlacement for GivenViews {
        fn view_requested(
            &self,
            program: &RunningProgram,
            request: ViewRequest,
        ) -> Result<(), ViewError> {
            let none = Insets {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            };
            let window =
                matches!(self, GivenViews::InWindows).then(|| Arc::new(FilledWindow::default()));
            let view = program.add_view(PlacedView {
                view: Arc::new(KeptPicture::default()),
                text_input: Some(Arc::new(NoInputMethods)),
                window: window.as_ref().map(|window| PlacedWindow {
                    window: Arc::clone(window) as Arc<dyn ShellWindow>,
                    frame: screen().frame,
                    maximized: false,
                    title_bar_double_click_action: None,
                }),
                metrics: Metrics {
                    width: WINDOW_WIDTH,
                    height: WINDOW_HEIGHT,
                    device_pixel_ratio: 2.0,
                    padding: none,
                    view_padding: none,
                    view_insets: none,
                },
                screen: Some(SCREEN),
                presence: Presence {
                    on_screen: true,
                    occluded: false,
                    focused: true,
                },
                request: Some(request.id),
            });
            if let Some(window) = window {
                *window.0.lock().unwrap() = Some(view);
            }
            Ok(())
        }
    }

    /// A window's view that keeps the picture presented last until the next replaces it.
    #[derive(Default)]
    struct KeptPicture(Mutex<Option<Box<dyn FnOnce() + Send + Sync>>>);

    impl ShellView for KeptPicture {
        fn present(
            &self,
            _buffer: HardwareBuffer,
            _layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
            released: Box<dyn FnOnce() + Send + Sync>,
        ) {
            let before = self.0.lock().unwrap().replace(released);
            if let Some(before) = before {
                before();
            }
        }

        fn set_cursor(&self, _cursor: Cursor) {}

        fn request_focus_change(&self, _state: ViewFocusState, _direction: ViewFocusDirection) {}

        fn set_handles_back(&self, _handles: bool) {}

        fn set_overlay_style(&self, _style: OverlayStyle) {}
    }

    /// Input methods that do nothing of their own.
    struct NoInputMethods;

    impl ShellTextInput for NoInputMethods {
        fn start(&self, _configuration: TextInputConfiguration) {}
        fn set_editing_state(&self, _value: TextEditingValue) {}
        fn set_caret_rect(&self, _rect: PixelRect) {}
        fn set_composing_rect(&self, _rect: PixelRect) {}
        fn show(&self) {}
        fn hide(&self) {}
        fn stop(&self) {}
    }

    /// A window its view fills, with nothing around it, which takes every frame it is given and
    /// reports it at once, with its view's size at 2x; otherwise it stays as it is, whatever the
    /// program asks of it. It holds its view until it is closed.
    #[derive(Default)]
    struct FilledWindow(Mutex<Option<ProgramView>>);

    impl ShellWindow for FilledWindow {
        fn set_frame(&self, frame: Rect, _animate: Option<Duration>) {
            let Some(view) = self.0.lock().unwrap().clone() else {
                return;
            };
            view.window_frame_changed(frame);
            view.metrics_changed(Metrics {
                width: (frame.size.width * 2.0).round() as u32,
                height: (frame.size.height * 2.0).round() as u32,
                ..view.metrics()
            });
        }
        fn set_title(&self, _title: &str) {}
        fn show(&self) {}
        fn hide(&self) {}
        fn close(&self) {
            self.0.lock().unwrap().take();
        }
        fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits> {
            None
        }
        fn drag(&self) {}
        fn set_maximized(&self, _maximized: bool) {}
        fn set_minimized(&self, _minimized: bool) {}
    }

    /// The screen's refreshes, each [`REFRESH_INTERVAL`] after it was asked, reported to the engine
    /// once it is made.
    #[derive(Default)]
    struct Refreshes {
        engine: OnceLock<Weak<WaOS>>,
    }

    impl VsyncWaiter for Refreshes {
        fn next_vsync(&self, screen: u64) {
            let Some(engine) = self.engine.get().cloned() else {
                return;
            };
            thread::spawn(move || {
                thread::sleep(REFRESH_INTERVAL);
                if let Some(engine) = engine.upgrade() {
                    let now = Instant::now();
                    engine.screen_refreshed(screen, now, now + REFRESH_INTERVAL);
                }
            });
        }
    }
}
