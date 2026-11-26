//! One program's view, shown on the main thread, between the program and Waft: it carries out what
//! the program asks of the view, and tells the program what happens to it. It knows nothing of a
//! window: whatever shows the view, a window of its own (`view_windows`) or a tab of WaOS's deck,
//! carries out the program's calls on a window where it has one, and says what the view's box
//! measures and how the view stands.
//!
//! The program's calls arrive in the order it made them: the cursor, and the style of the
//! system's bars over the view, which the view's canvas shows, so the view notifies; whether the
//! bars hide, which the view tells the bars over the Waft view it is shown in, while it may hold
//! the keyboard, as the tab in front of WaOS's deck does, and which it shows again once it may
//! not or is gone; whether the
//! program handles Back, which the view keeps for
//! whatever shows it to read as the user's Back comes; the focus and the text input, which only
//! the canvas can carry out, passed on to it as events; and the window's, passed on as events to
//! whatever shows the view in a window, and to nothing where none does. The view's metrics, its screen and its
//! presence go to the view's link, each only when it changed. Whatever shows the view also says
//! whether it may hold the keyboard: a view put out of its user's reach, as a tab of WaOS's deck
//! shrunk to a card is, gives the input focus up, and takes none at its program's request until
//! it may again.
//!
//! The invariant: the view notifies only when its cursor or its bars' style changes, so its canvas
//! rebuilds for nothing else; frames never pass through here.

use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waft::ui::{
    CompositionLayer, SystemBarsBehavior as WaftSystemBarsBehavior, SystemUiOverlayStyle,
    ViewFocusState, ViewId, ViewMetrics, ViewRef,
};
use waft::{App, Context, Entity, EventEmitter, WeakEntity};
use waos::Presence;
use waos::bindings::wasdk::platform::views::{
    Cursor, OverlayStyle, SystemBarsBehavior, ViewFocusState as ProgramFocusState,
};
use waos_window_messages::{SystemBarsCall, TextInputCall, ViewCall, WindowCall};

use crate::link::ViewLink;
use crate::logic::canvas_layer::CanvasLayer;
use crate::logic::geometry::metrics_of;
use crate::logic::overlay_style::{said_over, waft_overlay_style};

pub struct ShownView {
    view: Rc<dyn ViewLink>,
    canvas: Arc<CanvasLayer>,
    waft_view: ViewRef,
    cursor: Cursor,
    /// Whether the program goes back within itself when the user asks, as it said last.
    handles_back: bool,
    /// The style of the system's bars over the view, each field as the program said it last;
    /// none until it says one.
    overlay_style: Option<OverlayStyle>,
    /// Whether the program hid the bars over the view, and what a swipe from an edge does then;
    /// none while it wants them shown.
    bars_hidden: Option<SystemBarsBehavior>,
    /// What the bars over the Waft view were last told: hidden, and how, or none for shown.
    bars_told: Option<SystemBarsBehavior>,
    /// What the program was last told of how the view stands.
    presence: Presence,
    /// What the program was last told of the view's metrics, as Waft reported them.
    metrics: ViewMetrics,
    /// Whether whatever shows the view lets it hold the keyboard.
    holds_keyboard: bool,
}

/// The program asked for its view to have the input focus, or not to: its canvas's to do.
pub(crate) struct FocusChangeRequested(pub(crate) ViewFocusState);

impl EventEmitter<FocusChangeRequested> for ShownView {}

/// A call on the view's text input, which the canvas's text input carries out.
impl EventEmitter<TextInputCall> for ShownView {}

/// A call on the view's window, which whatever shows the view in a window carries out.
impl EventEmitter<WindowCall> for ShownView {}

impl ShownView {
    /// `view`, its canvas's layer `canvas` drawn in `waft_view`, in a box `metrics` measures,
    /// standing as `presence` says, carrying out the calls that arrive on `calls` from now on.
    pub fn open(
        app: &mut App,
        view: Rc<dyn ViewLink>,
        canvas: Arc<CanvasLayer>,
        waft_view: ViewRef,
        metrics: ViewMetrics,
        presence: Presence,
        calls: UnboundedReceiver<ViewCall>,
    ) -> Entity<ShownView> {
        let shown = app.new_entity(|_| ShownView {
            view,
            canvas,
            waft_view,
            cursor: Cursor::Default,
            handles_back: false,
            overlay_style: None,
            bars_hidden: None,
            bars_told: None,
            presence,
            metrics,
            holds_keyboard: true,
        });
        carry_out_calls(app, shown.downgrade(), calls);
        shown
    }

    /// Where the view's events go.
    pub(crate) fn view(&self) -> &dyn ViewLink {
        self.view.as_ref()
    }

    /// The composition layer the canvas shows.
    pub(crate) fn layer(&self) -> Arc<dyn CompositionLayer> {
        self.canvas.layer()
    }

    pub(crate) fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// The Waft view the canvas is drawn in.
    pub(crate) fn waft_view(&self) -> ViewId {
        self.waft_view.id()
    }

    /// Physical pixels per logical one where the view is now.
    pub(crate) fn device_pixel_ratio(&self) -> f64 {
        self.metrics.device_pixel_ratio
    }

    /// Carries out what the program asked of the view.
    fn carry_out(&mut self, call: ViewCall, cx: &mut Context<ShownView>) {
        tracing::trace!(?call, "a call on a view");
        match call {
            ViewCall::SetCursor(cursor) => self.set_cursor(cursor, cx),
            ViewCall::RequestFocusChange(state, _direction) => {
                let state = focus_state_of(state);
                if self.holds_keyboard || state == ViewFocusState::Unfocused {
                    cx.emit(FocusChangeRequested(state));
                }
            }
            ViewCall::SetHandlesBack(handles) => self.handles_back = handles,
            ViewCall::TextInput(call) => cx.emit(call),
            ViewCall::SystemBars(SystemBarsCall::SetOverlayStyle(style)) => {
                self.set_overlay_style(style, cx);
            }
            ViewCall::SystemBars(SystemBarsCall::Hide(behavior)) => self.hide_bars(Some(behavior)),
            ViewCall::SystemBars(SystemBarsCall::Show) => self.hide_bars(None),
            ViewCall::Window(call) => cx.emit(call),
        }
    }

    /// The program hid the bars over the view, or showed them again where `hidden` is none.
    fn hide_bars(&mut self, hidden: Option<SystemBarsBehavior>) {
        self.bars_hidden = hidden;
        self.tell_the_bars();
    }

    /// Tells the bars over the Waft view what the program said, while the view may hold the
    /// keyboard, and to show while it may not; nothing where that is what they were told last, or
    /// where no bars are drawn over the Waft view.
    fn tell_the_bars(&mut self) {
        let wanted = self.bars_hidden.filter(|_| self.holds_keyboard);
        if wanted == self.bars_told {
            return;
        }
        self.bars_told = wanted;
        let Some(bars) = self.waft_view.host().system_bars() else {
            return;
        };
        match wanted {
            Some(behavior) => bars.hide(waft_system_bars_behavior(behavior)),
            None => bars.show(),
        }
    }

    /// The style of the system's bars over the view, in Waft's terms, as the program said it;
    /// none while it has said none, and the bars keep what whatever shows the view gives them.
    pub(crate) fn overlay_style(&self) -> Option<SystemUiOverlayStyle> {
        self.overlay_style.as_ref().map(waft_overlay_style)
    }

    /// Whether the program goes back within itself when the user asks, as it said last: while it
    /// does, whatever shows the view sends it the user's Back (`pop_route`); while it does not,
    /// does its own with the press.
    pub fn handles_back(&self) -> bool {
        self.handles_back
    }

    /// The user asked to go back, and the program handles that: it hears so.
    pub fn pop_route(&self) {
        self.view.pop_route();
    }

    fn set_overlay_style(&mut self, style: OverlayStyle, cx: &mut Context<ShownView>) {
        let said = match self.overlay_style {
            Some(before) => said_over(before, style),
            None => style,
        };
        let changed = self.overlay_style() != Some(waft_overlay_style(&said));
        self.overlay_style = Some(said);
        if changed {
            cx.notify();
        }
    }

    fn set_cursor(&mut self, cursor: Cursor, cx: &mut Context<ShownView>) {
        if self.cursor != cursor {
            self.cursor = cursor;
            cx.notify();
        }
    }

    /// The view's box measures `metrics` now, in Waft's terms: the program hears it, and its
    /// screen, if they changed.
    pub fn set_metrics(&mut self, metrics: ViewMetrics) {
        if metrics != self.metrics {
            self.metrics = metrics;
            self.view
                .screen_changed(metrics.screen.map(|screen| screen.0));
            self.view.metrics_changed(metrics_of(&metrics));
        }
    }

    /// The program asked its window for a frame the window did not take: it hears the view's
    /// metrics as they are, its answer.
    pub fn answer_frame_request(&mut self) {
        self.view.metrics_changed(metrics_of(&self.metrics));
    }

    /// The view stands as `presence` says now: the program hears it if it changed.
    pub fn set_presence(&mut self, presence: Presence) {
        if presence != self.presence {
            self.presence = presence;
            self.view.presence_changed(presence);
        }
    }

    /// Whether the view may hold the keyboard.
    pub(crate) fn holds_keyboard(&self) -> bool {
        self.holds_keyboard
    }

    /// The view may hold the keyboard from now on, or may not: its canvas takes the input focus
    /// as it may again, and gives it up as it may no longer.
    pub fn set_holds_keyboard(&mut self, holds: bool, cx: &mut Context<ShownView>) {
        if holds == self.holds_keyboard {
            return;
        }
        self.holds_keyboard = holds;
        self.tell_the_bars();
        let state = if holds {
            ViewFocusState::Focused
        } else {
            ViewFocusState::Unfocused
        };
        cx.emit(FocusChangeRequested(state));
    }

    /// Nothing shows the view any more: what the program presents goes straight back, and the
    /// bars it hid over the Waft view show again.
    pub fn gone(&self) {
        self.canvas.window_gone();
        if self.bars_told.is_some()
            && let Some(bars) = self.waft_view.host().system_bars()
        {
            bars.show();
        }
    }
}

fn waft_system_bars_behavior(behavior: SystemBarsBehavior) -> WaftSystemBarsBehavior {
    match behavior {
        SystemBarsBehavior::ShowBarsBySwipe => WaftSystemBarsBehavior::ShowBarsBySwipe,
        SystemBarsBehavior::ShowTransientBarsBySwipe => {
            WaftSystemBarsBehavior::ShowTransientBarsBySwipe
        }
    }
}

fn focus_state_of(state: ProgramFocusState) -> ViewFocusState {
    match state {
        ProgramFocusState::Focused => ViewFocusState::Focused,
        ProgramFocusState::Unfocused => ViewFocusState::Unfocused,
    }
}

/// One task carries out every call the program makes on the view, on the main thread, for as
/// long as the program can make them and the view is shown.
fn carry_out_calls(
    app: &mut App,
    shown: WeakEntity<ShownView>,
    mut calls: UnboundedReceiver<ViewCall>,
) {
    app.spawn(async move |cx| {
        while let Some(call) = calls.next().await {
            let Some(shown) = shown.upgrade() else {
                break;
            };
            cx.update(|app| shown.update(app, |shown, cx| shown.carry_out(call, cx)));
        }
    });
}
