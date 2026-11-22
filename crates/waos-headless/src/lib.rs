//! A screen with no display, as the engine's shell parts: one screen, each program's request for a
//! view given one at once, in a window filling the screen or in none, focused and in sight; frames
//! refreshed by one thread of its own, as soon as asked or a set time after; and each view keeping
//! the picture presented last until the next replaces it, which the engine reads back
//! (`ProgramView::picture`). What `waos --headless` runs on, and the tests that run a program.
//!
//! The screen is the shell's placement and refreshes alone, with the prompts its maker gives it:
//! every other part of the shell is its maker's to plug in, real or a test's own.
//!
//! The invariants a later editor will break: nothing here touches a window system; a view's last
//! picture is released only once the next is presented, so a screenshot can always read it; and
//! a refresh reaches the engine only from the screen's own thread, never from inside the engine's
//! call that asked for it.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use waos::bindings::wasdk::platform::screens::PixelSize;
use waos::bindings::wasdk::platform::views::{
    Cursor, Insets, Metrics, OverlayStyle, PixelRect, Placement, RequestId, TextEditingValue,
    TextInputConfiguration, ViewError, ViewFocusDirection, ViewFocusState,
};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};
use waos::{
    HardwareBuffer, OnRefresh, PlacedView, PlacedWindow, Presence, ProgramView, Prompts,
    RunningProgram, Screen, Shell, ShellCompositionLayer, ShellTextInput, ShellView, ShellWindow,
    ShellWindowSizeLimits, ViewPlacement, ViewRequest, VsyncWaiter,
};

/// The screen's identifier.
pub const SCREEN: u64 = 1;

/// The screen's size in physical pixels and its scale, and whether a view is given in a window, as
/// on a desktop, or in none, as a phone's deck shows a tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub windows: bool,
}

/// When the screen refreshes after the engine asks it to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refreshes {
    /// At once: what a test runs on, which waits on nothing but what the program does.
    WhenAsked,
    /// This long after: what a program animating on a headless server is paced by.
    Every(Duration),
}

/// A screen with no display: its views' placement and its refreshes, as the engine's shell parts.
pub struct HeadlessScreen {
    placement: Arc<GivenViews>,
    refresher: Arc<Refresher>,
}

impl HeadlessScreen {
    /// A screen of `display`, refreshing as `refreshes` says.
    pub fn new(display: Display, refreshes: Refreshes) -> HeadlessScreen {
        let interval = match refreshes {
            Refreshes::WhenAsked => Duration::ZERO,
            Refreshes::Every(interval) => interval,
        };
        HeadlessScreen {
            placement: Arc::new(GivenViews { display }),
            refresher: Arc::new(Refresher::start(interval)),
        }
    }

    /// The screen as the engine sees it, which its `PlatformState` lists.
    pub fn screen(&self) -> Screen {
        self.placement.display.screen()
    }

    /// A shell of this screen's placement and refreshes, asking its questions of `prompts`, with
    /// every other part none, for its maker to plug in.
    pub fn shell(&self, prompts: Arc<dyn Prompts>) -> Shell {
        Shell {
            views: Arc::clone(&self.placement) as Arc<dyn ViewPlacement>,
            vsync: Arc::clone(&self.refresher) as Arc<dyn VsyncWaiter>,
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

    /// Gives `program` a view for its request `request`, as the screen gives one for each request:
    /// what a test that places a view itself calls.
    pub fn add_view(&self, program: &RunningProgram, request: RequestId) -> ProgramView {
        self.placement.add_view(program, request)
    }
}

impl Display {
    /// The screen of this display, refreshing at 60 Hz.
    fn screen(self) -> Screen {
        let frame = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: self.points(),
        };
        Screen {
            id: SCREEN,
            name: "Headless".into(),
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

    /// The display's size in logical points.
    fn points(self) -> Size {
        Size {
            width: f64::from(self.width) / self.scale,
            height: f64::from(self.height) / self.scale,
        }
    }

    /// What a view filling the display measures.
    fn metrics(self) -> Metrics {
        let none = Insets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        };
        Metrics {
            width: self.width,
            height: self.height,
            device_pixel_ratio: self.scale,
            padding: none,
            view_padding: none,
            view_insets: none,
        }
    }
}

/// Each request given a view at once, filling the display, focused and in sight.
struct GivenViews {
    display: Display,
}

impl GivenViews {
    fn add_view(&self, program: &RunningProgram, request: RequestId) -> ProgramView {
        let display = self.display;
        let window = display
            .windows
            .then(|| Arc::new(FilledWindow::new(display.scale)));
        let view = program.add_view(
            request,
            PlacedView {
                view: Arc::new(KeptPicture::default()),
                text_input: Some(Arc::new(NoInputMethods)),
                window: window.as_ref().map(|window| PlacedWindow {
                    window: Arc::clone(window) as Arc<dyn ShellWindow>,
                    frame: display.screen().frame,
                    maximized: false,
                    title_bar_double_click_action: None,
                }),
                metrics: display.metrics(),
                screen: Some(SCREEN),
                presence: Presence {
                    on_screen: true,
                    occluded: false,
                    focused: true,
                },
            },
        );
        if let Some(window) = window {
            *locked(&window.view) = Some(view.clone());
        }
        view
    }
}

impl ViewPlacement for GivenViews {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        self.add_view(program, request.id);
        Ok(())
    }
}

/// A view that keeps the picture presented last until the next replaces it.
#[derive(Default)]
struct KeptPicture(Mutex<Option<Box<dyn FnOnce() + Send + Sync>>>);

impl ShellView for KeptPicture {
    fn present(
        &self,
        _buffer: HardwareBuffer,
        _layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        let before = locked(&self.0).replace(released);
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
        let Some(view) = locked(&self.view).clone() else {
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
        locked(&self.view).take();
    }

    fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits> {
        None
    }

    fn drag(&self) {}
    fn set_maximized(&self, _maximized: bool) {}
    fn set_minimized(&self, _minimized: bool) {}
}

/// The screen's refreshes, each `interval` after it was asked, on a thread of the screen's own,
/// which ends with the screen.
struct Refresher {
    asks: Sender<OnRefresh>,
}

impl Refresher {
    /// Starts the thread, which answers each refresh asked `interval` later.
    fn start(interval: Duration) -> Refresher {
        let (asks, asked) = mpsc::channel::<OnRefresh>();
        thread::Builder::new()
            .name("headless screen".into())
            .spawn(move || {
                for on_refresh in asked {
                    thread::sleep(interval);
                    let now = Instant::now();
                    on_refresh(now, now + interval);
                }
            })
            .expect("a thread for the headless screen's refreshes");
        Refresher { asks }
    }
}

impl VsyncWaiter for Refresher {
    fn next_vsync(&self, _screen: u64, on_refresh: OnRefresh) {
        // The thread ends only with the screen, which is asking.
        let _ = self.asks.send(on_refresh);
    }
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
