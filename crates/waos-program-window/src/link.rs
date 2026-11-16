//! What a program's window needs of its program, wherever the program runs: [`ProgramLink`] for
//! the program, which its windows are asked for by and answered to, and [`ViewLink`] for each view
//! shown, which the window reports what happens to it to. In WaOS's process they are the engine's
//! own `RunningProgram` and `ProgramView` (`in_process`); in a program's window process they are
//! messages to WaOS.
//!
//! The invariant a later editor will break: nothing here waits on the program, but a resize. Every
//! call is one-way, or answered later through a future, as a key's reply is, so a window never
//! waits on another thread or another process; but a link may hold [`ViewLink::metrics_changed`]
//! until the program's picture at the new size is shown, as a program's window process does on
//! macOS (decision 55 of `docs/proposals/2026-10-17-the-waos-project.md`), so the window's new
//! size shows with it.

use std::sync::Arc;

use futures::channel::mpsc::UnboundedSender;
use waos::bindings::wasdk::platform::screens::ScreenId;
use waos::bindings::wasdk::platform::views::{
    EditCommand, KeyEvent, Metrics, PointerEvent, RequestId, TextEditingValue, TextInputAction,
    ViewError, ViewFocusEvent,
};
use waos::bindings::wasdk::platform::windows::{Rect, TitleBarAction};
use waos::{HeldModifiers, KeyHandledFuture, Presence, ViewRequest};
use waos_window_messages::ViewCall;

use crate::logic::canvas_layer::CanvasLayer;

/// One program, as its windows reach it. Two links are equal when they reach the same program.
pub trait ProgramLink: Clone + PartialEq + 'static {
    /// What a view's window tells the program of the view.
    type View: ViewLink + Clone + 'static;

    /// The name the program runs under, which its windows' initial data names it by
    /// (`RestorableWindow`).
    fn name(&self) -> &str;

    /// Resolves once the program has ended, when its windows close.
    fn ended(&self) -> impl Future<Output = ()> + 'static;

    /// Adds the view shown in the window just made for `request`, starting as `opened` says.
    fn add_view(&self, request: RequestId, opened: OpenedView) -> Self::View;

    /// The program hears that `request` got no window, and why.
    fn view_request_failed(&self, request: RequestId, error: ViewError);

    /// The window of `view` is gone, and the view with it: the program hears so.
    fn remove_view(&self, view: &Self::View);
}

/// One view of a program, as its window reports what happens to it: the methods of the engine's
/// `ProgramView` the shell reports through, under the same names.
pub trait ViewLink {
    /// A pointer event over the view, with the modifier keys the keyboard holds as it happens.
    fn pointer(&self, event: PointerEvent, modifiers: &HeldModifiers);

    /// A key, while the view has the keyboard's focus; answers later whether the program handled
    /// it.
    fn key(&self, event: KeyEvent) -> KeyHandledFuture;

    /// The view gained or lost the input focus.
    fn focus(&self, event: ViewFocusEvent);

    /// An input method changed the text: `value` is the whole of it after the change.
    fn text_input_editing_value(&self, value: TextEditingValue);

    /// The user asked for the text input's configured action.
    fn text_input_action(&self, action: TextInputAction);

    /// The platform ended editing.
    fn text_input_closed(&self);

    /// Edit's Copy, for what is selected in the view.
    fn copy(&self);

    /// Edit's Cut, for what is selected in the view.
    fn cut(&self);

    /// Edit's Paste, into the view.
    fn paste(&self);

    /// Edit's Undo, Redo, Select All or Delete, in the view.
    fn edit(&self, command: EditCommand);

    /// The user asked to close the window; the program decides.
    fn close_requested(&self);

    /// The user asked to go back, and the program said it handles that.
    fn pop_route(&self);

    /// The view's geometry changed: called inside the platform's resize, before it shows the new
    /// size, which a link may hold until the program's picture at that size is shown.
    fn metrics_changed(&self, metrics: Metrics);

    /// The view is on another screen, or on none.
    fn screen_changed(&self, screen: Option<ScreenId>);

    /// The view stands on screen differently: shown or not, focused or not.
    fn presence_changed(&self, presence: Presence);

    /// The window the view is in moved or changed size.
    fn window_frame_changed(&self, frame: Rect);

    /// The window the view is in is maximized now, on macOS zoomed, or no longer.
    fn window_maximized_changed(&self, maximized: bool);

    /// The system's settings say a double-click on a title bar does `action` now; none where the
    /// system has no such setting.
    fn title_bar_double_click_action_changed(&self, action: Option<TitleBarAction>);
}

/// A view as its window opens, which [`ProgramLink::add_view`] adds.
pub struct OpenedView {
    /// The composition layer the view's canvas shows, placed by its window's frames.
    pub canvas: Arc<CanvasLayer>,
    /// Where the program's calls on the view go: the window's queue, on the main thread.
    pub calls: UnboundedSender<ViewCall>,
    pub metrics: Metrics,
    /// The screen the view is on; none while on no screen.
    pub screen: Option<ScreenId>,
    pub presence: Presence,
    /// Where the window is on the desktop, in logical points from the top-left of the main
    /// screen, y down.
    pub frame: Rect,
    /// Whether the window has size limits, which the program's calls on it then set: not where
    /// users do not resize windows.
    pub size_limits: bool,
    /// Whether the window is maximized; on macOS, zoomed.
    pub maximized: bool,
    /// What the system's settings say a double-click on a title bar does; none where the system
    /// has no such setting.
    pub title_bar_double_click_action: Option<TitleBarAction>,
}

/// One program's request for a window, on its way to the main thread of the process its windows
/// are in.
pub struct WindowRequest<P> {
    pub program: P,
    pub request: ViewRequest,
    pub face: ProgramFace,
}

/// The program as its windows show it where the system shows a window's own title and icon, as
/// Android's Recents shows a task's: the name its user knows it by, on a window it left untitled,
/// and its icon; and, where its windows are an app of their own, which app.
#[derive(Clone, Debug, Default)]
pub struct ProgramFace {
    pub name: String,
    pub icon: Option<waft::ui::IconImage>,
    /// The app the system takes the program's windows for; the process's where `None`.
    pub identity: Option<ProgramIdentity>,
}

/// The app a program's windows are to a system that tells a window's app by the window, not by its
/// process, as Windows' taskbar and Linux's docks do: its own button there, grouping its own
/// windows, which can be pinned.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgramIdentity {
    /// Its id with the system: the windows' AppUserModelID on Windows, their app id on Linux,
    /// which the desktop matches to the desktop entry of that name.
    pub app_id: String,
    /// What a pin of one of its windows starts, on Windows, where no Start menu shortcut carries
    /// its id.
    pub relaunch: Option<waft::WindowsRelaunch>,
}
