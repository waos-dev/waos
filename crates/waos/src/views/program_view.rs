//! One view's state, as the shell reports into it from any thread and the engine reads it: what
//! the view is (its id, the request it arrived for, what that request attached), where it is (its
//! geometry, which a tool that resizes its window waits on, its screen, how it stands on screen,
//! its window), whether a tool watches its program, which keeps it in sight though occluded (the
//! design's decision 63), the shell's side of it,
//! which the program's requests go to, whether the program goes back within itself when the user
//! asks, the events on their way to the program, the size the
//! program set its canvas to, whether it waits for a frame, the refresh it had its last frame of,
//! how many of its frames are in flight, and the picture it shows with the layers beneath it
//! (`shown_picture`), which a tool waits on and takes a screenshot of (`screenshots`).
//!
//! A view gets one frame for each refresh of the screen it is on, as an observer of Chromium's
//! `BeginFrameSource` gets one begin-frame for each tick: a refresh is an interval a view's frame
//! is taken of once, not an instant. A frame asked for while the view has not had the frame of
//! the refresh its screen is in is handed over at once, with that refresh's times, as an observer
//! added between two ticks is sent the missed one; one asked for after the view had that frame
//! is handed over when the next refresh comes. So a frame shows what reached the program during
//! its own refresh, whichever of the two reached it first. Either way the view is in sight, on
//! screen and not occluded unless a tool watches its program, and fewer than two of its frames
//! are in flight (`frames_in_flight`); a view that is not asks for its screen's next refresh, or
//! once it can be served again. Where the engine does not know the refresh a screen is in
//! (`screen_refreshes`), every frame waits for the next. Which views of a program a refresh that
//! comes serves is the program's views' to decide (`program_views`).
//!
//! Every clone is the same view. The invariant: a report changes what the program reads back
//! before it queues what the program hears, so the two never disagree; a change of presence
//! reaches the program's views, which derive the program's state from every view's, so the
//! lifecycle never hears of a view on its own; a screen's refresh is asked for a view from here
//! alone, the refresh after one that served a frame being the engine's own to ask
//! (`screen_refreshes`), and a frame request is never lost between the program's thread and the
//! shell's: the request is recorded before the view's screen, presence and watchers are read,
//! and any of them is changed before the request is read, so whichever comes second asks; and
//! every key event the program
//! hears through the view, reported or synthesized, is recorded in the program's record of the
//! modifier keys it was told of, which all its views share, and queued under that record's lock,
//! so the record and what the program hears keep one order (the queue's lock is taken inside the
//! record's, never the other way); and the shell is told of each change of whether the program
//! handles Back, and of nothing else, so what it was told last is what the program said last.
//!
//! The steps every frame takes through here are recorded as [`targets::FRAMES`] events naming
//! the view: the program asks for it, at a refresh it is handed over, naming the views it serves,
//! and once the GPU has drawn it, it is handed to the shell.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use tokio::sync::watch;

use crate::bindings::wasdk::platform::hardware_buffers::{ImageError, PixelBuffer};
use crate::bindings::wasdk::platform::screens::ScreenId;
use crate::bindings::wasdk::platform::views::{
    EditCommand, Frame, KeyEvent, Metrics, Placement, PointerEvent, RequestId, TextEditingValue,
    TextInputAction, ViewEvent, ViewFocusEvent, ViewId,
};
use crate::bindings::wasdk::platform::windows::{Rect, Size, TitleBarAction};
use crate::clipboard::{CopyRequest, PlatformClipboard};
use crate::clock::Clock;
use crate::hardware_buffers::Released;
use crate::lifecycle::{Lifecycle, Presence};
use crate::screenshots::Screenshots;
use crate::shell::{HardwareBuffer, PlacedView, ShellCompositionLayer, ShellView};
use crate::sync::locked;
use crate::targets;
use crate::windows::ProgramWindow;

use super::event_queue::{EventQueue, QueuedEvent, ViewEvents};
use super::frames_in_flight::{FrameInFlight, FramesInFlight};
use super::key_reply::{KeyHandledFuture, KeyReply};
use super::modifiers::{HeldModifiers, ToldModifiers};
use super::program_views::{Requested, ViewsState};
use super::screen_refreshes::{Refresh, ScreenRefreshes};
use super::shown_picture::{HandedPicture, ShownPicture};
use super::view_text_input::ViewTextInput;

/// One view, shared by the shell, which reports into it, and the engine, which reads it.
#[derive(Clone)]
pub struct ProgramView(Arc<ViewState>);

struct ViewState {
    id: ViewId,
    request_id: Option<RequestId>,
    initial_data: Option<String>,
    clock: Clock,
    /// The modifier keys the program has been told are held, by this view or another of its.
    told_modifiers: Arc<Mutex<ToldModifiers>>,
    shell: Arc<dyn ShellView>,
    text_input: Option<ViewTextInput>,
    window: Option<ProgramWindow>,
    /// The view's geometry as last reported, which a tool that resizes its window waits on.
    metrics: watch::Sender<Metrics>,
    screen: Mutex<Option<ScreenId>>,
    presence: Mutex<Presence>,
    /// How many tools watch the program now, which every view of it shares.
    watchers: Arc<AtomicUsize>,
    events: EventQueue,
    /// Whether the program goes back within itself when the user asks, as it said last.
    handles_back: AtomicBool,
    /// The size of the textures the view's canvas hands out, as the program set it; none before
    /// it sets one.
    canvas_size: Mutex<Option<[u32; 2]>>,
    /// Whether the program waits for a frame.
    frame_requested: AtomicBool,
    /// When the refresh the view had its last frame of began; none before its first frame. Held
    /// while a frame is taken, so a refresh that comes and a request between two take one each.
    last_refresh: Mutex<Option<Instant>>,
    /// The frames the program presented that are not handed to the shell yet.
    frames_in_flight: FramesInFlight,
    /// The last picture handed to the shell, with the layers beneath it.
    shown: ShownPicture,
    /// What draws the view's screenshot.
    screenshots: Arc<Screenshots>,
    /// The screens' refreshes the engine waits for, which a frame asked for waits on.
    refreshes: ScreenRefreshes,
    /// The platform's clipboard, which a paste is of.
    clipboard: PlatformClipboard,
    /// The program's views, which derive its state from every view's presence; gone once the
    /// program has ended.
    views: Weak<ViewsState>,
}

/// The frame of `refresh` for `views`, stamped with the refresh's times on the program's `clock`.
pub(super) fn frame_of(refresh: Refresh, clock: &Clock, views: Vec<ViewId>) -> Frame {
    Frame {
        time: clock.at(refresh.at),
        display_time: clock.at(refresh.display_time),
        views,
    }
}

/// What a view is made with besides what the shell placed: what the program shares among its
/// views and the engine among every program's.
pub(super) struct Shared {
    pub(super) clock: Clock,
    /// The program's record of the modifier keys it has been told are held.
    pub(super) told_modifiers: Arc<Mutex<ToldModifiers>>,
    /// How many tools watch the program now.
    pub(super) watchers: Arc<AtomicUsize>,
    pub(super) refreshes: ScreenRefreshes,
    /// The platform's clipboard, which a paste is of.
    pub(super) clipboard: PlatformClipboard,
    /// What draws a view's screenshot.
    pub(super) screenshots: Arc<Screenshots>,
    pub(super) views: Weak<ViewsState>,
}

impl ProgramView {
    /// The view `placed`, with the id `id` and what the program asked for it with, sharing
    /// `shared` with the program's other views.
    pub(super) fn new(
        id: ViewId,
        placed: PlacedView,
        requested: Requested,
        shared: Shared,
    ) -> ProgramView {
        let title = requested.title;
        ProgramView(Arc::new(ViewState {
            id,
            request_id: placed.request,
            initial_data: requested.initial_data,
            clock: shared.clock,
            told_modifiers: shared.told_modifiers,
            shell: placed.view,
            text_input: placed.text_input.map(ViewTextInput::new),
            window: placed
                .window
                .map(|window| ProgramWindow::new(window, title)),
            metrics: watch::Sender::new(placed.metrics),
            screen: Mutex::new(placed.screen),
            presence: Mutex::new(placed.presence),
            watchers: shared.watchers,
            events: EventQueue::default(),
            handles_back: AtomicBool::new(false),
            canvas_size: Mutex::new(None),
            frame_requested: AtomicBool::new(false),
            last_refresh: Mutex::new(None),
            frames_in_flight: FramesInFlight::default(),
            shown: ShownPicture::new(),
            screenshots: shared.screenshots,
            refreshes: shared.refreshes,
            clipboard: shared.clipboard,
            views: shared.views,
        }))
    }

    pub fn id(&self) -> ViewId {
        self.0.id
    }

    /// The request this view arrived for; none for a view the shell gave.
    pub fn request_id(&self) -> Option<RequestId> {
        self.0.request_id
    }

    /// What the request this view arrived for attached; none for a view the shell gave, or a
    /// request that attached nothing.
    pub(crate) fn initial_data(&self) -> Option<&str> {
        self.0.initial_data.as_deref()
    }

    /// The view's geometry as last reported.
    pub fn metrics(&self) -> Metrics {
        *self.0.metrics.borrow()
    }

    /// The screen the view is on now; none while it is on no screen.
    pub fn screen(&self) -> Option<ScreenId> {
        *locked(&self.0.screen)
    }

    /// How the view stands on screen, as last reported.
    pub fn presence(&self) -> Presence {
        *locked(&self.0.presence)
    }

    /// How the view stands as the engine counts it, which its program's state follows: as last
    /// reported, but never occluded while a tool watches the program.
    pub(super) fn counted_presence(&self) -> Presence {
        let presence = self.presence();
        if self.0.watchers.load(Ordering::SeqCst) == 0 {
            return presence;
        }
        Presence {
            occluded: false,
            ..presence
        }
    }

    /// Whether the view is in sight, which its frames are served by: on screen, and not occluded
    /// unless a tool watches its program. A minimized window is out of sight, watched or not.
    pub fn in_sight(&self) -> bool {
        self.counted_presence().in_sight()
    }

    /// Now on the program's clock, in nanoseconds: what the shell stamps the pointer and key
    /// events it reports on this view with, so the program compares them with the time it reads
    /// itself.
    pub fn now(&self) -> u64 {
        self.0.clock.now()
    }

    /// The window the view is in; none where the platform shows it without one.
    pub(crate) fn window(&self) -> Option<&ProgramWindow> {
        self.0.window.as_ref()
    }

    /// Sizes the window the view is in so the view is `size` logical points, its top-left where it
    /// is: through the shell's window, as the program's `set-frame` goes, so each placement answers
    /// as it answers the program (`ProgramWindow::size_view`). What a tool that tries a program's
    /// layout at another size asks (the design's decision 60). Resolves with the view's metrics
    /// at the shell's next report of them, which a window system may have clamped, or with none
    /// once the view is gone; a window left as it was reports nothing. None for a view in no
    /// window, a phone's, whose size is the platform's.
    pub fn resize_window(
        &self,
        size: Size,
    ) -> Option<impl Future<Output = Option<Metrics>> + Send + 'static + use<>> {
        let window = self.0.window.as_ref()?;
        let mut reports = self.0.metrics.subscribe();
        let metrics = *reports.borrow_and_update();
        let scale = metrics.device_pixel_ratio;
        let view = Size {
            width: f64::from(metrics.width) / scale,
            height: f64::from(metrics.height) / scale,
        };
        window.size_view(view, size);
        Some(async move {
            reports.changed().await.ok()?;
            Some(*reports.borrow())
        })
    }

    /// The title the program gave the window the view is in, last: empty for one it left
    /// untitled, which the shell titles after the program; none for a view in no window.
    pub fn window_title(&self) -> Option<String> {
        self.0.window.as_ref().map(ProgramWindow::title)
    }

    /// The shell's side of the view, which the program's requests go to.
    pub(crate) fn shell(&self) -> &dyn ShellView {
        self.0.shell.as_ref()
    }

    /// The platform's input methods for the view; none where it has no text input.
    pub(crate) fn text_input(&self) -> Option<&ViewTextInput> {
        self.0.text_input.as_ref()
    }

    /// What the view's input methods edit now: the value the program set or an input method
    /// changed last, which a tool typing into the view commits its text into (the design's
    /// decision 60); none while nothing in the view is edited, and where it has no text input.
    pub fn editing_value(&self) -> Option<TextEditingValue> {
        self.0.text_input.as_ref()?.editing_value()
    }

    /// The view's geometry changed: the program reads it from here on, and hears about it.
    pub fn metrics_changed(&self, metrics: Metrics) {
        self.0.metrics.send_replace(metrics);
        self.0
            .events
            .push(QueuedEvent::Plain(ViewEvent::Metrics(metrics)));
    }

    /// The view is on `screen` now, or on no screen: a frame it waits for waits for that
    /// screen's refresh.
    pub fn screen_changed(&self, screen: Option<ScreenId>) {
        *locked(&self.0.screen) = screen;
        self.ask_refresh_if_waiting();
    }

    /// The view stands on screen as `presence` says: its program's state follows, and a frame
    /// it waits for is served once it is in sight.
    pub fn presence_changed(&self, presence: Presence) {
        *locked(&self.0.presence) = presence;
        if let Some(views) = self.0.views.upgrade() {
            views.state_follows_views();
        }
        self.ask_refresh_if_waiting();
    }

    /// A tool began or stopped watching the program: a frame the view waits for is served once it
    /// is in sight.
    pub(super) fn watchers_changed(&self) {
        self.ask_refresh_if_waiting();
    }

    /// The window the view is in has the frame `frame` now. Nothing for a view in no window.
    pub fn window_frame_changed(&self, frame: Rect) {
        if let Some(window) = self.reported_window("a frame") {
            window.frame_changed(frame);
        }
    }

    /// The window the view is in is maximized now, on macOS zoomed, or no longer. Nothing for a
    /// view in no window.
    pub fn window_maximized_changed(&self, maximized: bool) {
        if let Some(window) = self.reported_window("whether it is maximized") {
            window.maximized_changed(maximized);
        }
    }

    /// The system's settings say a double-click on the title bar of the window the view is in
    /// does `action` now; none where the system has no such setting. Nothing for a view in no
    /// window.
    pub fn title_bar_double_click_action_changed(&self, action: Option<TitleBarAction>) {
        if let Some(window) = self.reported_window("a title bar's double-click") {
            window.title_bar_double_click_action_changed(action);
        }
    }

    /// The window the shell reports `what` of; none, and said so, for a view in no window.
    fn reported_window(&self, what: &str) -> Option<&ProgramWindow> {
        if self.0.window.is_none() {
            tracing::warn!(view = self.0.id, what, "reported for a view in no window");
        }
        self.0.window.as_ref()
    }

    /// `modifiers`: the modifier keys the keyboard holds as it happens. The engine first tells
    /// the program of any it hasn't heard, so a shift-click reads as one.
    ///
    /// Those keys are stamped with the pointer event's time: it revealed them, and they come
    /// just before it.
    pub fn pointer(&self, event: PointerEvent, modifiers: &HeldModifiers) {
        let mut told_modifiers = locked(&self.0.told_modifiers);
        for missed in told_modifiers.catch_up(modifiers, event.time) {
            self.0
                .events
                .push(QueuedEvent::Key(missed, KeyReply::unheard()));
        }
        self.0
            .events
            .push(QueuedEvent::Plain(ViewEvent::Pointer(event)));
    }

    /// A key, while the view has the keyboard's focus; answers whether the program handled it,
    /// not handled when the program drops its reply unanswered or never reads the key. A modifier
    /// key counts for every view of the program: a pointer event on another catches up only what
    /// the program did not hear.
    pub fn key(&self, event: KeyEvent) -> KeyHandledFuture {
        let (reply, handled) = KeyReply::awaited();
        let mut told_modifiers = locked(&self.0.told_modifiers);
        told_modifiers.record(&event);
        self.0.events.push(QueuedEvent::Key(event, reply));
        handled
    }

    /// The view gained or lost the input focus.
    pub fn focus(&self, event: ViewFocusEvent) {
        self.0
            .events
            .push(QueuedEvent::Plain(ViewEvent::Focus(event)));
    }

    /// An input method changed the text: `value` is the whole of it after the change.
    pub fn text_input_editing_value(&self, value: TextEditingValue) {
        if let Some(text_input) = &self.0.text_input {
            text_input.edited(&value);
        }
        self.0
            .events
            .push(QueuedEvent::Plain(ViewEvent::TextInputEditingValue(value)));
    }

    /// The user asked for the configured action.
    pub fn text_input_action(&self, action: TextInputAction) {
        self.0
            .events
            .push(QueuedEvent::Plain(ViewEvent::TextInputAction(action)));
    }

    /// The platform ended editing.
    pub fn text_input_closed(&self) {
        if let Some(text_input) = &self.0.text_input {
            text_input.closed();
        }
        self.0
            .events
            .push(QueuedEvent::Plain(ViewEvent::TextInputClosed));
    }

    /// The user copied what is selected in the view, through the platform's command: the program
    /// writes it with the request it hears, whatever its write switch says, once.
    pub fn copy(&self) {
        self.0
            .events
            .push(QueuedEvent::Copy(CopyRequest::default()));
    }

    /// The user cut what is selected in the view, through the platform's command: the program
    /// writes it as a copy does, then deletes it.
    pub fn cut(&self) {
        self.0.events.push(QueuedEvent::Cut(CopyRequest::default()));
    }

    /// The user pasted into the view, through the platform's command: the program reads the
    /// clipboard as it is now, whatever its read switch says, until anything replaces it.
    pub fn paste(&self) {
        self.0
            .events
            .push(QueuedEvent::Paste(self.0.clipboard.pasted()));
    }

    /// The user asked to undo, redo, select everything or delete what is selected, through the
    /// platform's command.
    pub fn edit(&self, command: EditCommand) {
        self.0
            .events
            .push(QueuedEvent::Plain(ViewEvent::Edit(command)));
    }

    /// The user asked to close what holds the view; the program decides, thawed until it has
    /// handled the request, since it decides whether the program goes on.
    pub fn close_requested(&self) {
        let event = QueuedEvent::Plain(ViewEvent::CloseRequested);
        match self.0.views.upgrade() {
            Some(views) => self.0.events.push_waking(event, views.lifecycle()),
            None => self.0.events.push(event),
        }
    }

    /// Whether the program goes back within itself when the user asks, as it said last: while it
    /// does, the shell sends the user's Back to it (`pop_route`); while it does not, the shell does
    /// its own with the press.
    pub fn handles_back(&self) -> bool {
        self.0.handles_back.load(Ordering::SeqCst)
    }

    /// The program says whether it goes back within itself: the shell is told where that changed.
    pub(crate) fn set_handles_back(&self, handles: bool) {
        if self.0.handles_back.swap(handles, Ordering::SeqCst) != handles {
            self.0.shell.set_handles_back(handles);
        }
    }

    /// The user asked to go back, and the program handles that: it hears so. The shell sends it
    /// only while the program says it handles Back.
    pub fn pop_route(&self) {
        self.0.events.push(QueuedEvent::Plain(ViewEvent::PopRoute));
    }

    /// The size the program draws the view's pictures at, as it set it last; none before it set
    /// one. Whatever the view's own size: the canvas never follows the view.
    pub(crate) fn canvas_size(&self) -> Option<[u32; 2]> {
        *locked(&self.0.canvas_size)
    }

    /// The program sets the size of the textures the view's canvas hands out, from the next on.
    pub(crate) fn set_canvas_size(&self, size: [u32; 2]) {
        *locked(&self.0.canvas_size) = Some(size);
    }

    /// The view's events, from its arrival on; `None` once they were taken.
    pub(crate) fn events(&self) -> Option<ViewEvents> {
        self.0.events.take()
    }

    /// The program asks for one frame of the screen the view is on: the frame of the refresh the
    /// screen is in, at once, where the view has not had it, and the next refresh's otherwise,
    /// which is asked for unless the view is out of sight or on no screen. Asking again before
    /// the frame arrives changes nothing.
    pub(crate) fn request_frame(&self) {
        if self.0.frame_requested.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::trace!(target: targets::FRAMES, view = self.0.id, "the program asks for a frame");
        if !self.takes_frame_of_current_refresh() {
            self.ask_refresh_if_waiting();
        }
    }

    /// Hands the view the frame of the refresh its screen is in, alone, if the engine knows that
    /// refresh and the view takes its frame; the refresh after is then the engine's to hear of.
    fn takes_frame_of_current_refresh(&self) -> bool {
        let Some(screen) = self.screen_serving_frames() else {
            return false;
        };
        let Some(refresh) = self.0.refreshes.current(screen, Instant::now()) else {
            return false;
        };
        if !self.takes_frame_of(screen, refresh) {
            return false;
        }
        self.hand_over(frame_of(refresh, &self.0.clock, vec![self.0.id]));
        self.0.refreshes.frame_served(screen);
        true
    }

    /// Whether the program is waiting for a frame.
    pub(crate) fn wants_frame(&self) -> bool {
        self.0.frame_requested.load(Ordering::SeqCst)
    }

    /// Asks for the next refresh of the screen the view is on, if the program waits for a frame
    /// and a refresh there can serve it.
    pub(super) fn ask_refresh_if_waiting(&self) {
        if !self.0.frame_requested.load(Ordering::SeqCst) {
            return;
        }
        if let Some(screen) = self.screen_serving_frames() {
            self.0.refreshes.ask(screen);
        }
    }

    /// The screen whose refreshes serve the view's frames: the one it is on, while it is in
    /// sight.
    fn screen_serving_frames(&self) -> Option<ScreenId> {
        if !self.in_sight() {
            return None;
        }
        self.screen()
    }

    /// Whether the view takes the frame of `refresh` of `screen` now, which answers its request:
    /// the program waits for a frame, the view is in sight on that screen with fewer than two
    /// frames in flight, and `refresh` began after the one it had its last frame of, as
    /// Chromium's `CheckBeginFrameContinuity` asks of a begin-frame. One with two in flight is
    /// left out, as Flutter's `Animator::BeginFrame` tries again at the next frame interval while
    /// its pipeline is full. A view left out still waits: who asked sees to the next refresh.
    pub(super) fn takes_frame_of(&self, screen: ScreenId, refresh: Refresh) -> bool {
        if self.screen_serving_frames() != Some(screen) || !self.wants_frame() {
            return false;
        }
        let mut last_refresh = locked(&self.0.last_refresh);
        if last_refresh.is_some_and(|last| refresh.at <= last) {
            return false;
        }
        if self.0.frames_in_flight.full() {
            tracing::trace!(
                target: targets::FRAMES,
                view = self.0.id,
                "two frames are in flight: the frame waits for the next refresh"
            );
            return false;
        }
        if !self.0.frame_requested.swap(false, Ordering::SeqCst) {
            return false;
        }
        *last_refresh = Some(refresh.at);
        true
    }

    /// The program presented a frame, in flight until what is answered is dropped: once the
    /// frame is handed to the shell, or lost on the way.
    pub(crate) fn frame_in_flight(&self) -> FrameInFlight {
        self.0.frames_in_flight.begin()
    }

    /// Hands `frame` over, after every event before it.
    pub(super) fn hand_over(&self, frame: Frame) {
        tracing::trace!(
            target: targets::FRAMES,
            view = self.0.id,
            views = ?frame.views,
            "the frame is handed over"
        );
        self.0
            .events
            .push(QueuedEvent::Plain(ViewEvent::Frame(frame)));
    }

    /// The GPU has drawn a frame into `buffer`, which the program presented after reading
    /// `events_read` of the view's events: it is handed to the shell with `layers` beneath it, and
    /// is the picture the view shows once the shell has it; `released`, the pool's release of it,
    /// runs once the shell is done with it and the next picture has replaced it.
    pub(crate) fn present(
        &self,
        buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Released,
        events_read: u64,
    ) {
        tracing::trace!(
            target: targets::FRAMES,
            view = self.0.id,
            layers = layers.len(),
            "the GPU has drawn the frame: it is handed to the shell"
        );
        let (picture, released) =
            HandedPicture::new(&buffer, layers.clone(), released, events_read);
        self.0.shell.present(buffer, layers, released);
        self.0.shown.show(picture);
    }

    /// How many of the view's events the program has read.
    pub(crate) fn events_read(&self) -> u64 {
        self.0.events.read()
    }

    /// What the view shows, at its size: the last picture it handed the shell, which the shell
    /// shows from its compositor's next frame on, over what each layer it was handed with shows
    /// now, drawn together on the GPU (`screenshots`); BGRA, premultiplied, in rows of
    /// `row_bytes`. None before the first picture, once the view is gone, and while it has no
    /// size. What a tool that looks at the program's window reads (the design's decisions 60 and
    /// 62). Blocks while the GPU draws it.
    pub fn picture(&self) -> Result<Option<PixelBuffer>, ImageError> {
        let Some(shown) = self.0.shown.now() else {
            return Ok(None);
        };
        let metrics = self.metrics();
        if metrics.width == 0 || metrics.height == 0 {
            return Ok(None);
        }
        let size = [metrics.width, metrics.height];
        let picture = self
            .0
            .screenshots
            .take(size, shown.drawing(), &shown.layers())?;
        Ok(Some(picture))
    }

    /// Resolves with true once the view shows a picture the program presented after reading every
    /// event the view has queued now: the first that can show what they did, as an input a tool
    /// gives awaits before it looks; with false once the view is gone first. A program that draws
    /// nothing after them shows no such picture, and a view out of sight is served no frame.
    pub fn picture_after_events(&self) -> impl Future<Output = bool> + Send + 'static + use<> {
        self.0.shown.after_events(self.0.events.queued())
    }

    /// The clock the view's frames are stamped on: the program's.
    pub(crate) fn clock(&self) -> Clock {
        self.0.clock
    }

    /// The shell took the view away: the program reads what is left of its events, and then they
    /// end, the program, whose lifecycle is `lifecycle`, thawed until it has read the end, since
    /// the view's going decides whether it goes on; it shows nothing more.
    pub(super) fn end(&self, lifecycle: &Lifecycle) {
        self.0.events.end(lifecycle);
        self.0.shown.end();
    }
}

#[cfg(test)]
pub(super) mod tests {
    use crate::bindings::wasdk::platform::views::{KeyKind, PointerChange};
    use crate::bindings::wasdk::platform::windows::Point;
    use crate::shell::tests::Headless;
    use crate::shell::{PlacedWindow, ShellWindow};
    use crate::windows::tests::{placed_window, rect};

    use super::super::event_queue::tests::{Heard, heard, metrics, pointer};
    use super::super::modifiers::tests::{SHIFT, key_event, keyboard_holding};
    use super::super::screen_refreshes::tests::{
        A_SECOND, refresh, refresh_at, refreshes_a_second_apart, refreshes_of,
    };
    use super::*;

    pub(in crate::views) const ON_SCREEN: Presence = Presence {
        on_screen: true,
        occluded: false,
        focused: false,
    };
    const OFF_SCREEN: Presence = Presence {
        on_screen: false,
        occluded: false,
        focused: false,
    };

    /// A view placed by `shell`, with `metrics(1)`, on no screen, for the request 7.
    pub(in crate::views) fn placed(shell: &Arc<Headless>) -> PlacedView {
        PlacedView {
            view: Arc::clone(shell) as Arc<dyn ShellView>,
            text_input: None,
            window: None,
            metrics: metrics(1),
            screen: None,
            presence: ON_SCREEN,
            request: Some(7),
        }
    }

    /// What a view of a program with no other view shares, its refreshes asked of `shell`.
    pub(in crate::views) fn shared_with(shell: &Arc<Headless>) -> Shared {
        Shared {
            clock: Clock::start(),
            told_modifiers: Arc::default(),
            watchers: Arc::default(),
            refreshes: refreshes_of(shell),
            clipboard: PlatformClipboard::new(None),
            screenshots: crate::screenshots::tests::screenshots(),
            views: Weak::new(),
        }
    }

    /// The view `placed` places, with the id 3, on `shell`.
    fn view_placed(shell: &Arc<Headless>, placed: PlacedView) -> ProgramView {
        let requested = Requested {
            initial_data: Some("opened at start".into()),
            title: "Notes".into(),
        };
        ProgramView::new(3, placed, requested, shared_with(shell))
    }

    fn view_on(shell: &Arc<Headless>) -> ProgramView {
        view_placed(shell, placed(shell))
    }

    /// A view on `shell`, on `screen`, standing as `presence` says.
    fn view_at(shell: &Arc<Headless>, screen: Option<ScreenId>, presence: Presence) -> ProgramView {
        let placed = PlacedView {
            screen,
            presence,
            ..placed(shell)
        };
        view_placed(shell, placed)
    }

    fn view() -> ProgramView {
        view_on(&Arc::default())
    }

    /// What the view delivered, once it is gone.
    fn delivered(view: &ProgramView) -> Vec<Heard> {
        let events = view.events().expect("the events are taken once");
        view.end(&Lifecycle::default());
        heard(events)
    }

    #[test]
    fn a_view_answers_what_it_arrived_with() {
        let view = view();
        assert_eq!(view.id(), 3);
        assert_eq!(view.request_id(), Some(7));
        assert_eq!(view.initial_data(), Some("opened at start"));
        assert_eq!(view.metrics().width, 1);
        assert_eq!(view.screen(), None);
        assert_eq!(view.presence(), ON_SCREEN);
        assert!(view.window().is_none() && view.text_input().is_none());
    }

    /// A canvas never follows its view: a framework that configured a size draws at it.
    #[test]
    fn the_canvas_is_the_size_the_program_set_whatever_the_views() {
        let view = view();
        assert_eq!(
            view.canvas_size(),
            None,
            "no size until the program sets one"
        );
        view.set_canvas_size([640, 480]);
        view.metrics_changed(metrics(800));
        assert_eq!(view.canvas_size(), Some([640, 480]));
        view.set_canvas_size([800, 1]);
        assert_eq!(view.canvas_size(), Some([800, 1]));
    }

    #[test]
    fn a_refresh_of_the_views_screen_serves_one_frame_per_request() {
        let view = view_at(&Arc::default(), Some(1), ON_SCREEN);
        assert!(!view.takes_frame_of(1, refresh(0)), "nothing was asked for");
        view.request_frame();
        assert!(
            !view.takes_frame_of(2, refresh(0)),
            "another screen's refresh serves nothing"
        );
        assert!(view.takes_frame_of(1, refresh(0)));
        assert!(!view.wants_frame());
        assert!(
            !view.takes_frame_of(1, refresh(1)),
            "the request was answered once"
        );
    }

    #[test]
    fn a_view_takes_one_frame_of_a_refresh_and_none_of_one_before_it() {
        let view = view_at(&Arc::default(), Some(1), ON_SCREEN);
        view.request_frame();
        assert!(view.takes_frame_of(1, refresh(1)));
        view.request_frame();
        assert!(!view.takes_frame_of(1, refresh(1)), "it had this one's");
        assert!(!view.takes_frame_of(1, refresh(0)), "nor an earlier one's");
        assert!(view.wants_frame(), "its request waits");
        assert!(view.takes_frame_of(1, refresh(2)), "the next one's");
    }

    /// A view on the screen 1, which refreshes once a second, in a program of its own, and the
    /// refreshes its frames wait on.
    fn view_on_a_screen_refreshing_each_second(shell: &Arc<Headless>) -> ProgramView {
        let placed = PlacedView {
            screen: Some(1),
            ..placed(shell)
        };
        let shared = Shared {
            refreshes: refreshes_a_second_apart(shell),
            ..shared_with(shell)
        };
        ProgramView::new(3, placed, Requested::default(), shared)
    }

    /// The frames `view` was handed, once it is gone.
    fn frames_of(view: &ProgramView) -> Vec<Frame> {
        delivered(view)
            .into_iter()
            .map(|heard| match heard {
                Heard::Plain(ViewEvent::Frame(frame)) => frame,
                heard => panic!("only frames were handed over, not {heard:?}"),
            })
            .collect()
    }

    #[test]
    fn a_request_before_any_refresh_was_told_waits_for_the_next() {
        let shell = Arc::new(Headless::default());
        let view = view_on_a_screen_refreshing_each_second(&shell);
        view.request_frame();
        assert!(
            view.wants_frame(),
            "the refresh the screen is in is not known"
        );
        assert_eq!(shell.refreshes_asked(), [1]);
    }

    #[test]
    fn a_request_in_a_refresh_that_gave_the_view_nothing_is_served_at_once_with_its_times() {
        let shell = Arc::new(Headless::default());
        let view = view_on_a_screen_refreshing_each_second(&shell);
        let came = refresh_at(Instant::now());
        view.0.refreshes.came(1, came);
        view.request_frame();
        assert!(!view.wants_frame(), "served without waiting for a refresh");
        assert_eq!(
            shell.refreshes_asked(),
            [1],
            "the refresh after is the engine's to hear of"
        );
        let clock = view.clock();
        let [frame] = &frames_of(&view)[..] else {
            panic!("one frame");
        };
        assert_eq!(frame.time, clock.at(came.at));
        assert_eq!(frame.display_time, clock.at(came.display_time));
        assert_eq!(frame.views, [3], "the view alone");
    }

    #[test]
    fn a_request_after_the_view_had_the_refreshs_frame_waits_for_the_next_refresh() {
        let shell = Arc::new(Headless::default());
        let view = view_on_a_screen_refreshing_each_second(&shell);
        let came = refresh_at(Instant::now());
        view.0.refreshes.came(1, came);
        view.request_frame();
        view.request_frame();
        assert!(view.wants_frame(), "one frame of a refresh");
        assert!(!view.takes_frame_of(1, came), "which it had");
        assert!(view.takes_frame_of(1, refresh_at(came.at + A_SECOND)));
    }

    #[test]
    fn a_request_long_after_the_last_refresh_told_is_served_at_once_whole_intervals_on() {
        let shell = Arc::new(Headless::default());
        let view = view_on_a_screen_refreshing_each_second(&shell);
        let ten_and_a_half_ago = Instant::now() - A_SECOND * 10 - A_SECOND / 2;
        // Its picture expected late enough that the time is on the program's clock, which
        // started a moment ago.
        let long_ago = Refresh {
            at: ten_and_a_half_ago,
            display_time: ten_and_a_half_ago + A_SECOND * 11,
        };
        view.0.refreshes.came(1, long_ago);
        view.request_frame();
        assert!(!view.wants_frame(), "served at once");
        view.request_frame();
        let ten_on = refresh_at(long_ago.at + A_SECOND * 10);
        assert!(!view.takes_frame_of(1, ten_on), "the refresh it had");
        assert!(view.takes_frame_of(1, refresh_at(ten_on.at + A_SECOND)));
        let clock = view.clock();
        let [frame] = &frames_of(&view)[..] else {
            panic!("the frame handed over at once");
        };
        assert_eq!(
            frame.display_time,
            clock.at(long_ago.display_time + A_SECOND * 10),
            "as long after the refresh began as the last told"
        );
    }

    #[test]
    fn a_request_with_two_frames_in_flight_waits_for_the_next_refresh_though_it_had_none() {
        let shell = Arc::new(Headless::default());
        let view = view_on_a_screen_refreshing_each_second(&shell);
        view.0.refreshes.came(1, refresh_at(Instant::now()));
        let _in_flight = [view.frame_in_flight(), view.frame_in_flight()];
        view.request_frame();
        assert!(view.wants_frame(), "two in flight: no frame at once");
        assert_eq!(shell.refreshes_asked(), [1], "it asks for the next refresh");
    }

    #[test]
    fn a_request_of_a_view_out_of_sight_is_not_served_in_the_refresh_its_screen_is_in() {
        let shell = Arc::new(Headless::default());
        let view = view_on_a_screen_refreshing_each_second(&shell);
        view.presence_changed(OFF_SCREEN);
        view.0.refreshes.came(1, refresh_at(Instant::now()));
        view.request_frame();
        assert!(view.wants_frame(), "out of sight: no frame");
        assert!(shell.refreshes_asked().is_empty(), "and no refresh");
        view.presence_changed(ON_SCREEN);
        assert_eq!(shell.refreshes_asked(), [1], "seen: it asks for the next");
        assert!(view.wants_frame());
    }

    #[test]
    fn a_frame_handed_over_comes_after_the_events_before_it() {
        let view = view();
        view.close_requested();
        view.hand_over(Frame {
            time: 5,
            display_time: 21,
            views: vec![3],
        });
        let [
            Heard::Plain(ViewEvent::CloseRequested),
            Heard::Plain(ViewEvent::Frame(frame)),
        ] = &delivered(&view)[..]
        else {
            panic!("the close, then the frame");
        };
        assert_eq!((frame.time, &frame.views[..]), (5, &[3][..]));
    }

    #[test]
    fn the_shell_is_told_of_each_change_of_whether_the_program_handles_back() {
        let shell = Arc::new(Headless::default());
        let view = view_on(&shell);
        assert!(!view.handles_back(), "a view starts not handling Back");
        view.set_handles_back(false);
        view.set_handles_back(true);
        view.set_handles_back(true);
        assert!(view.handles_back());
        view.set_handles_back(false);
        assert_eq!(shell.handles_back(), [true, false], "only the changes");
    }

    #[test]
    fn the_users_back_reaches_the_program_in_order_with_the_rest() {
        let view = view();
        view.set_handles_back(true);
        view.pop_route();
        view.close_requested();
        let [
            Heard::Plain(ViewEvent::PopRoute),
            Heard::Plain(ViewEvent::CloseRequested),
        ] = &delivered(&view)[..]
        else {
            panic!("the Back, then the close");
        };
    }

    #[test]
    fn a_request_asks_for_the_refresh_of_the_views_screen_once_until_it_is_served() {
        let shell = Arc::new(Headless::default());
        let view = view_at(&shell, Some(1), ON_SCREEN);
        view.request_frame();
        view.request_frame();
        assert_eq!(shell.refreshes_asked(), [1]);
        assert!(view.takes_frame_of(1, refresh(0)));
        view.0.refreshes.came(1, refresh(0));
        view.request_frame();
        assert_eq!(
            shell.refreshes_asked(),
            [1, 1],
            "answered, the next request asks again"
        );
    }

    #[test]
    fn a_view_out_of_sight_waits_and_asks_once_it_is_seen() {
        let shell = Arc::new(Headless::default());
        let view = view_at(&shell, Some(1), OFF_SCREEN);
        view.request_frame();
        assert!(
            shell.refreshes_asked().is_empty(),
            "out of sight: no refresh"
        );
        assert!(!view.takes_frame_of(1, refresh(0)));
        view.presence_changed(ON_SCREEN);
        assert_eq!(shell.refreshes_asked(), [1], "seen: the request asks");
        assert!(view.takes_frame_of(1, refresh(1)));
    }

    #[test]
    fn a_view_covered_all_over_is_out_of_sight_and_waits() {
        let shell = Arc::new(Headless::default());
        let covered = Presence {
            occluded: true,
            ..ON_SCREEN
        };
        let view = view_at(&shell, Some(1), covered);
        assert!(!view.in_sight());
        view.request_frame();
        assert!(shell.refreshes_asked().is_empty(), "covered: no refresh");
        view.presence_changed(ON_SCREEN);
        assert!(view.in_sight());
        assert_eq!(shell.refreshes_asked(), [1], "uncovered: the request asks");
    }

    #[test]
    fn a_view_on_no_screen_waits_and_asks_the_screen_it_comes_to() {
        let shell = Arc::new(Headless::default());
        let view = view_at(&shell, None, ON_SCREEN);
        view.request_frame();
        assert!(shell.refreshes_asked().is_empty(), "no screen: no refresh");
        view.screen_changed(Some(2));
        assert_eq!(shell.refreshes_asked(), [2]);
        assert!(view.takes_frame_of(2, refresh(0)));
    }

    #[test]
    fn a_change_with_no_frame_asked_asks_for_nothing() {
        let shell = Arc::new(Headless::default());
        let view = view_at(&shell, None, OFF_SCREEN);
        view.screen_changed(Some(1));
        view.presence_changed(ON_SCREEN);
        assert!(shell.refreshes_asked().is_empty());
    }

    #[test]
    fn metrics_and_the_screen_are_read_back_and_metrics_announced() {
        let view = view();
        view.metrics_changed(metrics(2));
        view.screen_changed(Some(9));
        assert_eq!(view.metrics().width, 2);
        assert_eq!(view.screen(), Some(9));
        let [Heard::Plain(ViewEvent::Metrics(heard))] = &delivered(&view)[..] else {
            panic!("the change of geometry is the one event");
        };
        assert_eq!(heard.width, 2);
    }

    #[test]
    fn a_views_now_is_on_the_programs_clock() {
        let view = view();
        let before = view.0.clock.now();
        let now = view.now();
        assert!(before <= now && now <= view.0.clock.now());
    }

    #[test]
    fn a_modifier_the_program_missed_arrives_just_before_the_pointer_event_that_revealed_it() {
        let view = view();
        let click = PointerEvent {
            time: 42,
            ..pointer(PointerChange::Down, 0.0)
        };
        view.pointer(click, &keyboard_holding(&[SHIFT]));
        let [Heard::Key(shift), Heard::Plain(ViewEvent::Pointer(click))] = &delivered(&view)[..]
        else {
            panic!("the shift, then the click");
        };
        assert_eq!(
            (shift.kind, shift.physical, shift.logical),
            (KeyKind::Down, SHIFT.0, SHIFT.1)
        );
        assert!(shift.synthesized);
        assert_eq!((shift.time, click.time), (42, 42));
    }

    #[test]
    fn a_modifier_the_program_heard_is_not_told_again() {
        let view = view();
        let _handled = view.key(key_event(KeyKind::Down, SHIFT));
        view.pointer(
            pointer(PointerChange::Down, 0.0),
            &keyboard_holding(&[SHIFT]),
        );
        assert!(matches!(
            delivered(&view)[..],
            [
                Heard::Key(KeyEvent {
                    synthesized: false,
                    ..
                }),
                Heard::Plain(ViewEvent::Pointer(_))
            ]
        ));
    }

    #[test]
    fn a_presence_is_read_back() {
        let view = view();
        let presence = Presence {
            on_screen: true,
            occluded: false,
            focused: true,
        };
        view.presence_changed(presence);
        assert_eq!(view.presence(), presence);
    }

    /// A view of `shell`'s in a window of its.
    fn view_in_window(shell: &Arc<Headless>) -> ProgramView {
        let placed = PlacedView {
            window: Some(placed_window(Arc::clone(shell) as Arc<dyn ShellWindow>)),
            ..placed(shell)
        };
        view_placed(shell, placed)
    }

    #[test]
    fn a_window_frame_reported_is_read_back_from_the_window() {
        let view = view_in_window(&Arc::new(Headless::default()));
        view.window_frame_changed(rect(2.0));
        assert_eq!(view.window().unwrap().frame().origin.x, 2.0);
        assert_eq!(
            view.window_title().as_deref(),
            Some("Notes"),
            "titled as the program asked for it"
        );
    }

    #[test]
    fn whether_the_window_is_maximized_is_read_back_from_the_window() {
        let view = view_in_window(&Arc::new(Headless::default()));
        view.window_maximized_changed(true);
        assert!(view.window().unwrap().is_maximized());
    }

    #[test]
    fn a_title_bars_double_click_reported_is_read_back_from_the_window() {
        let view = view_in_window(&Arc::new(Headless::default()));
        view.title_bar_double_click_action_changed(Some(TitleBarAction::Minimize));
        assert_eq!(
            view.window().unwrap().title_bar_double_click_action(),
            Some(TitleBarAction::Minimize)
        );
    }

    #[test]
    fn a_view_in_no_window_is_not_resized() {
        let size = Size {
            width: 390.0,
            height: 844.0,
        };
        assert!(view().resize_window(size).is_none());
    }

    #[test]
    fn a_resize_goes_to_the_window_and_resolves_with_the_metrics_reported_next() {
        use futures::FutureExt as _;

        let shell = Arc::new(Headless::default());
        let at_2x = Metrics {
            width: 800,
            height: 600,
            device_pixel_ratio: 2.0,
            ..metrics(1)
        };
        let placed = PlacedView {
            window: Some(PlacedWindow {
                frame: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 400.0,
                        height: 300.0,
                    },
                },
                ..placed_window(Arc::clone(&shell) as Arc<dyn ShellWindow>)
            }),
            metrics: at_2x,
            ..placed(&shell)
        };
        let view = view_placed(&shell, placed);
        let size = Size {
            width: 195.0,
            height: 422.0,
        };
        let mut resized = Box::pin(view.resize_window(size).expect("a view in a window"));
        let set = shell.frames_set();
        assert_eq!(
            set.iter()
                .map(|frame| (frame.size.width, frame.size.height))
                .collect::<Vec<_>>(),
            [(195.0, 422.0)],
            "the view fills the window"
        );
        assert!(resized.as_mut().now_or_never().is_none(), "until reported");
        let reported = Metrics {
            width: 390,
            height: 844,
            ..at_2x
        };
        view.metrics_changed(reported);
        let metrics = resized.now_or_never().expect("reported").expect("a view");
        assert_eq!((metrics.width, metrics.height), (390, 844));
    }

    /// The shell reports from its threads and the program's thread reads.
    #[test]
    fn a_view_crosses_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<ProgramView>();
    }
}
