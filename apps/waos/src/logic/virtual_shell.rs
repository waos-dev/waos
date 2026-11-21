//! A virtual desktop shell with no real windows or compositor: one virtual screen refreshing on
//! a 16 ms timer, each view request given a window at once that keeps the picture presented last
//! until the next replaces it, and takes every frame it is given, reporting the new size at once.
//! Used by headless WaOS (`headless_entry`) and by the app's drawing tests (`test_engine::drawing`).
//!
//! The invariant: nothing here touches Waft, winit, Wayland, X11, or AppKit. Every view keeps its
//! last presented frame alive until replaced so screenshots can read it back cleanly.

use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use waos::bindings::wasdk::platform::screens::PixelSize;
use waos::bindings::wasdk::platform::views::{
    Cursor, Insets, Metrics, OverlayStyle, PixelRect, Placement, RequestId, TextEditingValue,
    TextInputConfiguration, ViewError, ViewFocusDirection, ViewFocusState,
};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};
use waos::{
    HardwareBuffer, PlacedView, PlacedWindow, Presence, ProgramView, Prompts, RunningProgram,
    Screen, Shell, ShellCompositionLayer, ShellTextInput, ShellView, ShellWindow,
    ShellWindowSizeLimits, ViewPlacement, ViewRequest, VsyncWaiter, WaOS,
};

/// The one virtual screen's identifier.
pub(crate) const VIRTUAL_SCREEN: u64 = 1;

/// How long after the engine asks the virtual screen refreshes (~60 Hz).
pub(crate) const REFRESH_INTERVAL: Duration = Duration::from_millis(16);

/// The virtual display's size and scale, in physical pixels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VirtualDisplay {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale: f64,
}

impl Default for VirtualDisplay {
    fn default() -> VirtualDisplay {
        VirtualDisplay {
            width: 1280,
            height: 800,
            scale: 1.0,
        }
    }
}

impl VirtualDisplay {
    /// The virtual screen matching this display's physical size and scale, refreshing at 60 Hz.
    pub(crate) fn screen(self) -> Screen {
        let points = Size {
            width: f64::from(self.width) / self.scale,
            height: f64::from(self.height) / self.scale,
        };
        let frame = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: points,
        };
        Screen {
            id: VIRTUAL_SCREEN,
            name: "Virtual Display".into(),
            size: PixelSize {
                width: self.width,
                height: self.height,
            },
            scale: self.scale,
            refresh_rate: 60_000,
            frame,
            visible_frame: frame,
        }
    }

    /// Opens a view for `program` in a window of its own, filling this display.
    #[cfg(test)]
    pub(crate) fn add_window(self, program: &RunningProgram, request: RequestId) -> ProgramView {
        self.add_view(program, request, true)
    }

    /// Opens a view for `program`, either in a window filling this display or windowless.
    pub(crate) fn add_view(
        self,
        program: &RunningProgram,
        request: RequestId,
        in_window: bool,
    ) -> ProgramView {
        let none = Insets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        };
        let window = in_window.then(|| Arc::new(FilledWindow::new(self.scale)));
        let view = program.add_view(
            request,
            PlacedView {
                view: Arc::new(KeptPicture::default()),
                text_input: Some(Arc::new(NoInputMethods)),
                window: window.as_ref().map(|window| PlacedWindow {
                    window: Arc::clone(window) as Arc<dyn ShellWindow>,
                    frame: self.screen().frame,
                    maximized: false,
                    title_bar_double_click_action: None,
                }),
                metrics: Metrics {
                    width: self.width,
                    height: self.height,
                    device_pixel_ratio: self.scale,
                    padding: none,
                    view_padding: none,
                    view_insets: none,
                },
                screen: Some(VIRTUAL_SCREEN),
                presence: Presence {
                    on_screen: true,
                    occluded: false,
                    focused: true,
                },
            },
        );
        if let Some(window) = window {
            *window.view.lock().unwrap() = Some(view.clone());
        }
        view
    }
}

/// Builds a [`Shell`] over a virtual display with `prompts` and a 16 ms [`Refreshes`] timer.
/// Call [`Refreshes::attach`] once the engine is constructed.
pub(crate) fn shell(
    display: VirtualDisplay,
    in_windows: bool,
    prompts: Arc<dyn Prompts>,
) -> (Shell, Arc<Refreshes>) {
    let refreshes = Arc::new(Refreshes::default());
    let shell = Shell {
        views: Arc::new(GivenViews {
            display,
            in_windows,
        }),
        vsync: Arc::clone(&refreshes) as Arc<dyn VsyncWaiter>,
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
    };
    (shell, refreshes)
}

/// Each request given a view at once, filling `display`, focused and in sight: in a window
/// that takes the frame it is given, or in none.
struct GivenViews {
    display: VirtualDisplay,
    in_windows: bool,
}

impl ViewPlacement for GivenViews {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        self.display.add_view(program, request.id, self.in_windows);
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
    fn set_client(&self, _configuration: TextInputConfiguration) {}
    fn show(&self) {}
    fn update_config(&self, _configuration: TextInputConfiguration) {}
    fn set_editing_state(&self, _value: TextEditingValue) {}
    fn set_caret_rect(&self, _rect: PixelRect) {}
    fn set_composing_rect(&self, _rect: PixelRect) {}
    fn clear_client(&self) {}
    fn hide(&self) {}
}

/// A window its view fills, with nothing around it, which takes every frame it is given and
/// reports it at once, scaled to physical pixels; otherwise it stays as it is, whatever the
/// program asks of it. It holds its view until it is closed.
struct FilledWindow {
    view: Mutex<Option<ProgramView>>,
    scale: f64,
}

impl FilledWindow {
    fn new(scale: f64) -> FilledWindow {
        FilledWindow {
            view: Mutex::new(None),
            scale,
        }
    }
}

impl ShellWindow for FilledWindow {
    fn set_frame(&self, frame: Rect, _animate: Option<Duration>) {
        let Some(view) = self.view.lock().unwrap().clone() else {
            return;
        };
        view.window_frame_changed(frame);
        view.metrics_changed(Metrics {
            width: (frame.size.width * self.scale).round() as u32,
            height: (frame.size.height * self.scale).round() as u32,
            ..view.metrics()
        });
    }

    fn set_title(&self, _title: &str) {}
    fn show(&self) {}
    fn hide(&self) {}

    fn close(&self) {
        self.view.lock().unwrap().take();
    }

    fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits> {
        None
    }

    fn drag(&self) {}
    fn set_maximized(&self, _maximized: bool) {}
    fn set_minimized(&self, _minimized: bool) {}
}

/// The screen's refreshes, each [`REFRESH_INTERVAL`] after it was asked, reported to the engine
/// once attached.
#[derive(Default)]
pub(crate) struct Refreshes {
    engine: OnceLock<Weak<WaOS>>,
}

impl Refreshes {
    /// Attaches `engine` so future refresh requests notify it.
    pub(crate) fn attach(&self, engine: &Arc<WaOS>) {
        let _ = self.engine.set(Arc::downgrade(engine));
    }
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
