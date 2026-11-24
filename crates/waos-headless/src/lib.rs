//! A headless shell: the windowing parts of the engine's shell, over one screen with no display.
//! Each program's request for a view is given one at once, in a window filling the screen or in
//! none, focused and in sight; frames are refreshed by one thread of the shell's own, as soon as
//! asked or a set time after; each view keeps the picture presented last until the next replaces
//! it, which the engine reads back (`ProgramView::picture`); and what the engine told each view and
//! its window, its title, its size limits, whether it is maximized, is kept for whoever reads it,
//! as a window system knows its windows. What `waos --headless` runs on, and the tests that run a
//! program. Every other part of the engine's shell, the questions it asks among them, is its
//! maker's to plug in, real or a test's own.
//!
//! The invariants a later editor will break: nothing here touches a window system; a view's last
//! picture is released only once the next is presented or the view is gone, so a screenshot can
//! always read it; the shell reaches its views only weakly, so it keeps nothing of a view the
//! engine let go of; and a refresh reaches the engine only from the shell's own thread, never from
//! inside the engine's call that asked for it.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::watch;
use waos::bindings::wasdk::platform::screens::PixelSize;
use waos::bindings::wasdk::platform::views::{
    Cursor, Insets, Metrics, OverlayStyle, PixelRect, Placement, TextEditingValue,
    TextInputConfiguration, TextInputType, ViewError, ViewFocusDirection, ViewFocusState,
};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};
use waos::{
    HardwareBuffer, OnRefresh, PlacedView, PlacedWindow, Presence, ProgramView, Prompts,
    RunningProgram, Screen, Shell, ShellCompositionLayer, ShellTextInput, ShellView, ShellWindow,
    ShellWindowSizeLimits, ViewPlacement, ViewRequest, VsyncWaiter, WeakProgramView, WindowConfig,
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
    /// At once: what a test runs on, whose program's frames come only as it asks for them.
    WhenAsked,
    /// This long after: what a program animating on a headless server is paced by.
    Every(Duration),
}

/// The windowing parts of the engine's shell, over a screen with no display.
pub struct HeadlessShell {
    views: Arc<GivenViews>,
    refresher: Arc<Refresher>,
}

impl HeadlessShell {
    /// A shell over a screen of `display`, refreshing as `refreshes` says.
    pub fn new(display: Display, refreshes: Refreshes) -> HeadlessShell {
        let interval = match refreshes {
            Refreshes::WhenAsked => Duration::ZERO,
            Refreshes::Every(interval) => interval,
        };
        HeadlessShell {
            views: Arc::new(GivenViews {
                display,
                given: Mutex::default(),
                count: watch::Sender::new(0),
            }),
            refresher: Arc::new(Refresher::start(interval)),
        }
    }

    /// The screen as the engine sees it, which its `PlatformState` lists.
    pub fn screen(&self) -> Screen {
        self.views.display.screen()
    }

    /// The engine's shell: this shell's placement and refreshes, asking its questions of
    /// `prompts`, with every other part none, for its maker to plug in.
    pub fn shell(&self, prompts: Arc<dyn Prompts>) -> Shell {
        Shell {
            views: Arc::clone(&self.views) as Arc<dyn ViewPlacement>,
            vsync: Arc::clone(&self.refresher) as Arc<dyn VsyncWaiter>,
            compositor: None,
            prompts,
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
            cameras: None,
            simulated_cameras: None,
            machine_picker: None,
        }
    }

    /// Every view the shell gave that is still open, the first given first.
    pub fn views(&self) -> Vec<Arc<HeadlessView>> {
        locked(&self.views.given)
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }

    /// Resolves once the shell has given its `index`th view, counting from 0, with it; with none
    /// where that view is gone by then.
    pub fn view(
        &self,
        index: usize,
    ) -> impl Future<Output = Option<Arc<HeadlessView>>> + Send + 'static + use<> {
        let views = Arc::clone(&self.views);
        let mut count = views.count.subscribe();
        async move {
            // The sender lives as long as `views`, held here.
            let _ = count.wait_for(|count| *count > index).await;
            locked(&views.given).get(index).and_then(Weak::upgrade)
        }
    }

    /// Gives `program` a view for its request `request`, as the shell gives one for each request:
    /// what a test that places a view itself calls.
    pub fn add_view(&self, program: &RunningProgram, request: ViewRequest) -> ProgramView {
        self.views.add_view(program, request)
    }
}

/// A view the shell gave a program, and what the engine told it since.
pub struct HeadlessView {
    view: OnceLock<WeakProgramView>,
    window: Option<Arc<HeadlessWindow>>,
    text_input: Arc<EditedText>,
    handles_back: AtomicBool,
    /// How many pictures the view was handed.
    pictures: AtomicUsize,
    /// The layers handed with the picture presented last, reached weakly, and where each goes.
    layers: Mutex<Vec<(Weak<dyn ShellCompositionLayer>, Placement)>>,
    /// The release of the picture presented last, which runs once the next replaces it.
    shown: Mutex<Option<Released>>,
}

/// What the engine hands the shell with a hardware buffer, to call once it is done showing it.
type Released = Box<dyn FnOnce() + Send + Sync>;

impl HeadlessView {
    /// The engine's view, while the program has it.
    pub fn view(&self) -> Option<ProgramView> {
        self.view.get()?.upgrade()
    }

    /// The window the view is in; none on a phone's deck.
    pub fn window(&self) -> Option<&HeadlessWindow> {
        self.window.as_deref()
    }

    /// Whether the program goes back within itself when the user asks, as it said last.
    pub fn handles_back(&self) -> bool {
        self.handles_back.load(Ordering::SeqCst)
    }

    /// How many pictures the view was handed: for a test that a view hidden while another kept
    /// drawing was handed none meanwhile.
    pub fn pictures_shown(&self) -> usize {
        self.pictures.load(Ordering::SeqCst)
    }

    /// The layers handed with the picture presented last, the bottom-most first, each where the
    /// program placed it; one the engine let go of since is left out.
    pub fn layers(&self) -> Vec<(Arc<dyn ShellCompositionLayer>, Placement)> {
        locked(&self.layers)
            .iter()
            .filter_map(|(layer, placement)| Some((layer.upgrade()?, placement.clone())))
            .collect()
    }

    /// The kind of text the program edits in the view now; none while it edits nothing.
    pub fn text_input_type(&self) -> Option<TextInputType> {
        *locked(&self.text_input.input_type)
    }
}

impl ShellView for HeadlessView {
    fn present(
        &self,
        _buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Released,
    ) {
        *locked(&self.layers) = layers
            .iter()
            .map(|(layer, placement)| (Arc::downgrade(layer), placement.clone()))
            .collect();
        self.pictures.fetch_add(1, Ordering::SeqCst);
        let before = locked(&self.shown).replace(released);
        if let Some(before) = before {
            before();
        }
    }

    fn set_cursor(&self, _cursor: Cursor) {}

    fn request_focus_change(&self, _state: ViewFocusState, _direction: ViewFocusDirection) {}

    fn set_handles_back(&self, handles: bool) {
        self.handles_back.store(handles, Ordering::SeqCst);
    }

    fn set_overlay_style(&self, _style: OverlayStyle) {}
}

/// The window around a view, filling the screen with nothing around the view, and what the engine
/// told it since. It takes every frame it is given and reports it at once, scaled to physical
/// pixels; closed, it takes its view away, as a window system's window closing does.
pub struct HeadlessWindow {
    view: OnceLock<WeakProgramView>,
    scale: f64,
    /// What the program asked for when it requested the view.
    config: WindowConfig,
    title: Mutex<String>,
    min_size: Mutex<Option<Size>>,
    max_size: Mutex<Option<Size>>,
    maximized: AtomicBool,
    minimized: AtomicBool,
    drags: AtomicUsize,
}

impl HeadlessWindow {
    /// What the program asked for when it requested the window's view.
    pub fn config(&self) -> &WindowConfig {
        &self.config
    }

    /// The title the window shows: the one the program asked for, or set last.
    pub fn title(&self) -> String {
        locked(&self.title).clone()
    }

    /// The smallest and the largest size the program last set, in logical points; none for one
    /// it has not set.
    pub fn size_limits(&self) -> (Option<Size>, Option<Size>) {
        (*locked(&self.min_size), *locked(&self.max_size))
    }

    /// Whether the program last asked for the window maximized.
    pub fn is_maximized(&self) -> bool {
        self.maximized.load(Ordering::SeqCst)
    }

    /// Whether the program last asked for the window minimized.
    pub fn is_minimized(&self) -> bool {
        self.minimized.load(Ordering::SeqCst)
    }

    /// How many moves of the window from a press the program asked for, as a window system starts
    /// one for a press on a title bar.
    pub fn drags(&self) -> usize {
        self.drags.load(Ordering::SeqCst)
    }

    fn view(&self) -> Option<ProgramView> {
        self.view.get()?.upgrade()
    }
}

impl ShellWindow for HeadlessWindow {
    fn set_frame(&self, frame: Rect, _animate: Option<Duration>) {
        let Some(view) = self.view() else {
            return;
        };
        view.window_frame_changed(frame);
        view.metrics_changed(Metrics {
            width: (frame.size.width * self.scale).round() as u32,
            height: (frame.size.height * self.scale).round() as u32,
            ..view.metrics()
        });
    }

    fn set_title(&self, title: &str) {
        *locked(&self.title) = title.to_owned();
    }

    fn show(&self) {}
    fn hide(&self) {}

    fn close(&self) {
        if let Some(view) = self.view() {
            view.remove();
        }
    }

    fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits> {
        Some(self)
    }

    fn drag(&self) {
        self.drags.fetch_add(1, Ordering::SeqCst);
    }

    fn set_maximized(&self, maximized: bool) {
        self.maximized.store(maximized, Ordering::SeqCst);
    }

    fn set_minimized(&self, minimized: bool) {
        self.minimized.store(minimized, Ordering::SeqCst);
    }
}

impl ShellWindowSizeLimits for HeadlessWindow {
    fn set_min_size(&self, size: Option<Size>) {
        *locked(&self.min_size) = size;
    }

    fn set_max_size(&self, size: Option<Size>) {
        *locked(&self.max_size) = size;
    }
}

impl Display {
    /// The screen of this display, refreshing at 60 Hz.
    fn screen(self) -> Screen {
        let frame = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: f64::from(self.width) / self.scale,
                height: f64::from(self.height) / self.scale,
            },
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

/// Each request given a view at once, filling the display, focused and in sight; and every view
/// given, reached weakly, in order.
struct GivenViews {
    display: Display,
    given: Mutex<Vec<Weak<HeadlessView>>>,
    /// How many views were given.
    count: watch::Sender<usize>,
}

impl GivenViews {
    fn add_view(&self, program: &RunningProgram, request: ViewRequest) -> ProgramView {
        let display = self.display;
        let window = display.windows.then(|| {
            Arc::new(HeadlessWindow {
                view: OnceLock::new(),
                scale: display.scale,
                title: Mutex::new(request.config.title.clone()),
                config: request.config,
                min_size: Mutex::default(),
                max_size: Mutex::default(),
                maximized: AtomicBool::new(false),
                minimized: AtomicBool::new(false),
                drags: AtomicUsize::new(0),
            })
        });
        let text_input = Arc::new(EditedText::default());
        let given = Arc::new(HeadlessView {
            view: OnceLock::new(),
            window: window.clone(),
            text_input: Arc::clone(&text_input),
            handles_back: AtomicBool::new(false),
            pictures: AtomicUsize::new(0),
            layers: Mutex::default(),
            shown: Mutex::default(),
        });
        let view = program.add_view(
            request.id,
            PlacedView {
                view: Arc::clone(&given) as Arc<dyn ShellView>,
                text_input: Some(text_input as Arc<dyn ShellTextInput>),
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
        let _ = given.view.set(view.downgrade());
        if let Some(window) = &window {
            let _ = window.view.set(view.downgrade());
        }
        locked(&self.given).push(Arc::downgrade(&given));
        self.count.send_modify(|count| *count += 1);
        view
    }
}

impl ViewPlacement for GivenViews {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        self.add_view(program, request);
        Ok(())
    }
}

/// Input methods that do nothing of their own, keeping the kind of text the program edits.
#[derive(Default)]
struct EditedText {
    input_type: Mutex<Option<TextInputType>>,
}

impl ShellTextInput for EditedText {
    fn set_client(&self, configuration: TextInputConfiguration) {
        *locked(&self.input_type) = Some(configuration.input_type);
    }

    fn show(&self) {}

    fn update_config(&self, configuration: TextInputConfiguration) {
        *locked(&self.input_type) = Some(configuration.input_type);
    }

    fn set_editing_state(&self, _value: TextEditingValue) {}
    fn set_caret_rect(&self, _rect: PixelRect) {}
    fn set_composing_rect(&self, _rect: PixelRect) {}

    fn clear_client(&self) {
        *locked(&self.input_type) = None;
    }

    fn hide(&self) {}
}

/// The screen's refreshes, each `interval` after it was asked, on a thread of the shell's own,
/// which ends with the shell.
struct Refresher {
    asks: Sender<OnRefresh>,
}

impl Refresher {
    /// Starts the thread, which answers each refresh asked `interval` later.
    fn start(interval: Duration) -> Refresher {
        let (asks, asked) = mpsc::channel::<OnRefresh>();
        thread::Builder::new()
            .name("headless refreshes".into())
            .spawn(move || {
                for on_refresh in asked {
                    thread::sleep(interval);
                    let now = Instant::now();
                    on_refresh(now, now + interval);
                }
            })
            .expect("a thread for the headless shell's refreshes");
        Refresher { asks }
    }
}

impl VsyncWaiter for Refresher {
    fn next_vsync(&self, _screen: u64, on_refresh: OnRefresh) {
        // The thread ends only with the shell, which is asking.
        let _ = self.asks.send(on_refresh);
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
