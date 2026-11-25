//! WaOS's in-process link: a program running in this process is reached through the engine's own
//! types, [`RunningProgram`] for the program and [`ProgramView`] for each of its views, under the
//! names the link's methods share with them. Its hardware buffers go to the canvas's surface from
//! the engine's frame threads, and its calls on a view to the view's queue on the main thread.

use std::sync::Arc;

use waos::bindings::wasdk::platform::screens::ScreenId;
use waos::bindings::wasdk::platform::views::{
    EditCommand, KeyEvent, Metrics, PointerEvent, RequestId, TextEditingValue, TextInputAction,
    ViewError, ViewFocusEvent,
};
use waos::bindings::wasdk::platform::windows::{Rect, TitleBarAction};
use waos::{
    HeldModifiers, KeyHandledFuture, PlacedView, PlacedWindow, Presence, ProgramView,
    RunningProgram,
};

use crate::link::{OpenedView, ProgramLink, ViewLink};
use crate::logic::canvas_view::CanvasView;
use crate::logic::view_calls::{ViewCalls, WindowCalls};

impl ProgramLink for RunningProgram {
    type View = ProgramView;

    fn name(&self) -> &str {
        RunningProgram::name(self)
    }

    fn ended(&self) -> impl Future<Output = ()> + 'static {
        let ended = RunningProgram::ended(self);
        async move {
            ended.await;
        }
    }

    /// The view, its hardware buffers presented to the canvas's surface and its calls queued for
    /// its window, carrying `request`.
    fn add_view(&self, request: RequestId, opened: OpenedView) -> ProgramView {
        RunningProgram::add_view(self, request, placed_view(opened))
    }

    fn view_request_failed(&self, request: RequestId, error: ViewError) {
        RunningProgram::view_request_failed(self, request, error);
    }

    fn remove_view(&self, view: &ProgramView) {
        RunningProgram::remove_view(self, view.id());
    }
}

/// The view `opened` opened in a window, as the engine is handed it, for a request or reserved for
/// one: its hardware buffers presented to the canvas's surface and its calls queued for its
/// window.
pub fn placed_view(opened: OpenedView) -> PlacedView {
    let calls = ViewCalls::queued(opened.calls);
    let canvas_view = CanvasView::new(opened.canvas, calls.clone());
    PlacedView {
        view: Arc::new(canvas_view),
        text_input: Some(Arc::new(calls.clone())),
        // A desktop's window has no system bars over it.
        system_bars: None,
        window: Some(PlacedWindow {
            window: Arc::new(WindowCalls::new(calls, opened.size_limits)),
            frame: opened.frame,
            maximized: opened.maximized,
            fullscreen: opened.fullscreen,
            title_bar_double_click_action: opened.title_bar_double_click_action,
        }),
        metrics: opened.metrics,
        screen: opened.screen,
        presence: opened.presence,
    }
}

impl ViewLink for ProgramView {
    fn pointer(&self, event: PointerEvent, modifiers: &HeldModifiers) {
        ProgramView::pointer(self, event, modifiers);
    }

    fn key(&self, event: KeyEvent) -> KeyHandledFuture {
        ProgramView::key(self, event)
    }

    fn focus(&self, event: ViewFocusEvent) {
        ProgramView::focus(self, event);
    }

    fn text_input_editing_value(&self, value: TextEditingValue) {
        ProgramView::text_input_editing_value(self, value);
    }

    fn text_input_action(&self, action: TextInputAction) {
        ProgramView::text_input_action(self, action);
    }

    fn text_input_closed(&self) {
        ProgramView::text_input_closed(self);
    }

    fn copy(&self) {
        ProgramView::copy(self);
    }

    fn cut(&self) {
        ProgramView::cut(self);
    }

    fn paste(&self) {
        ProgramView::paste(self);
    }

    fn edit(&self, command: EditCommand) {
        ProgramView::edit(self, command);
    }

    fn close_requested(&self) {
        ProgramView::close_requested(self);
    }

    fn pop_route(&self) {
        ProgramView::pop_route(self);
    }

    fn metrics_changed(&self, metrics: Metrics) {
        ProgramView::metrics_changed(self, metrics);
    }

    fn screen_changed(&self, screen: Option<ScreenId>) {
        ProgramView::screen_changed(self, screen);
    }

    fn presence_changed(&self, presence: Presence) {
        ProgramView::presence_changed(self, presence);
    }

    fn window_frame_changed(&self, frame: Rect) {
        ProgramView::window_frame_changed(self, frame);
    }

    fn window_maximized_changed(&self, maximized: bool) {
        ProgramView::window_maximized_changed(self, maximized);
    }

    fn window_fullscreen_changed(&self, fullscreen: bool) {
        ProgramView::window_fullscreen_changed(self, fullscreen);
    }

    fn title_bar_double_click_action_changed(&self, action: Option<TitleBarAction>) {
        ProgramView::title_bar_double_click_action_changed(self, action);
    }
}
