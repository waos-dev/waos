//! Each view a program asks for, in an Waft window of its own, on the main thread of the process
//! the windows are in. A request that arrives from the program is a request to Waft for a window
//! made as the program configured it, noted by what Waft answers it with; the window's view
//! arrives later, as every view does, and the program's view is added through its link with
//! everything it starts with (its metrics, its screen, how it stands, its window's frame, whether
//! the window is maximized and what a double-click on its title bar does, the request it answers),
//! or the request fails, with why. What goes in each window is built by whoever builds the
//! process's views, from the windows' content for it (`ProgramWindows::content_of`), which reports
//! each change of its window back here. From then on the windows keep each view's presence from
//! what Waft reports (the window's focus, each change of the window its content reports, which
//! says whether it is minimized or occluded, and each change of the app's lifecycle), and its
//! window's frame and the rest, carry out the program's calls on a window (its frame, its title,
//! its size limits, its move from a press, maximizing and minimizing among them), pass the user's
//! request to close a window on to the program, take the view away once Waft removed it, and close
//! the windows of a program that ended. They say which window is in front: the focused one, or else
//! the last to arrive. The view in a window is a `ShownView`, which knows nothing of the window.
//! A launch that asked for a program, as its pin does, brings its front window forward with the
//! launch's activation token on Linux, where a desktop lets a window take the focus only for the
//! launch it meant it for; a program with no window yet shows its next one with the token.
//!
//! Each window asked for carries, as its view's initial data, the program it shows and whether it
//! is the program's first window or an extra one (`logic::restoration`), so a window the system
//! restores to a process started afresh, as Android does a task from Recents, says what goes in it.
//! Such a window arrives as every view does, answering no request of this process, and whoever
//! hears of the platform's views hands it here as it arrives for its program to start with
//! (`open_given`); it is followed and shown as a window asked for is from then on. The user's Back
//! in a window goes to its program where the program handles Back, and otherwise sends the window
//! to the background, as Android sends an app's first screen there (decision 14 of
//! `docs/proposals/2026-11-12-phone-programs-and-library.md`); a desktop sends no Back.
//!
//! The invariant: every request taken is answered exactly once while its program runs, by
//! `add_view` or by `view_request_failed`, and every view added, asked for or given, is taken
//! away once with `remove_view` when Waft removes its view. The windows notify when one comes or
//! goes, so the content of each rebuilds then. A window's change reaches here from its content,
//! which rebuilds only while Waft builds frames, so every window is read again as the app's
//! lifecycle changes, its windows hidden with it. Whether a window is maximized is read only when
//! its frame changed: winit answers it for a window without decorations by giving the window a
//! title bar for a moment, which it reports as two moves and two resizes, so a read at each such
//! report would never stop (measured: every read reports a move and a resize).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use futures::channel::mpsc::UnboundedReceiver;
use futures::{FutureExt, StreamExt};
use waft::ui::{
    ViewError, ViewFocusDirection, ViewFocusEvent, ViewFocusState, ViewId, ViewMetrics, ViewRef,
};
use waft::{
    App, AppLifecycleState, Context, Entity, Subscription, WeakEntity, WidgetsBinding,
    WidgetsBindingObserver, WindowRef,
};
use waos::bindings::wasdk::platform::views::{RequestId, ViewError as ProgramViewError};
use waos::bindings::wasdk::platform::windows::{TitleBarAction, WindowBackground};
use waos_window_messages::{ViewCall, WindowCall};

use crate::link::{OpenedView, ProgramFace, ProgramLink, ViewLink, WindowRequest};
use crate::logic::canvas_layer::CanvasLayer;
use crate::logic::geometry::{metrics_of, rect_of, waft_rect, waft_size};
use crate::logic::presence::WindowStanding;
use crate::logic::restoration::{RestorableWindow, WindowRole};
use crate::logic::title_bar::title_bar_action_of;
use crate::logic::view_calls::view_call_queue;
use crate::logic::window_config::window_config_of;
use crate::state::shown_view::ShownView;

pub struct ViewWindows<P: ProgramLink> {
    /// The windows Waft is making, in the order they were asked for.
    awaiting: Vec<AwaitingWindow<P>>,
    /// The windows the platform gave, each waiting for the program starting with it.
    given: Vec<GivenWindow>,
    /// Each window shown, in the order they arrived.
    shown: Vec<ShownWindow<P>>,
    /// The activation token of a launch that asked for a program with no window, by the name it
    /// runs as, which its next window shows with.
    activation_tokens: HashMap<String, String>,
}

/// A window the platform gave, opened for a program about to start with it.
struct GivenWindow {
    view: ViewRef,
    window: WindowRef,
    canvas: Arc<CanvasLayer>,
    /// Where the program's calls on the view arrive, waiting until the program runs.
    calls: UnboundedReceiver<ViewCall>,
    standing: WindowStanding,
    /// What the program starts being told of the view.
    metrics: ViewMetrics,
}

/// A window Waft is making for a program's request.
struct AwaitingWindow<P> {
    /// What Waft answered the request for the window with, which its view arrives carrying.
    asked: waft::ui::RequestId,
    program: P,
    request: RequestId,
    /// What is behind the window shows through where the program draws nothing, but for a solid
    /// background.
    background: WindowBackground,
    visible: bool,
    activating: bool,
}

/// A window shown, with the program and the view it shows, and what the program was last told
/// of the window.
pub(crate) struct ShownWindow<P: ProgramLink> {
    pub(crate) program: P,
    /// The request the window was made for, which names it in what is traced; none for a window
    /// the platform gave.
    request: Option<RequestId>,
    /// The view as the program's link has it, which [`ViewWindows::waft_view_of`] picks by and
    /// the window reports to; the shown view reports to the same view.
    view: P::View,
    pub(crate) shown: Entity<ShownView>,
    /// The window's view, which the shown view fills.
    waft_view: ViewRef,
    window: WindowRef,
    standing: WindowStanding,
    /// What the program was last told of the window's frame.
    frame: waft::Rect,
    /// What the program was last told of whether the window is maximized.
    maximized: bool,
    /// What the program was last told a double-click on a title bar does.
    title_bar_double_click_action: Option<TitleBarAction>,
    /// Keeps the program's calls on the window coming from the shown view.
    _window_calls: Subscription,
}

impl<P: ProgramLink> ViewWindows<P> {
    /// The windows, which ask Waft for one for each request arriving on `requests`, and follow
    /// what Waft reports of the views they show.
    pub fn listen(
        app: &mut App,
        requests: UnboundedReceiver<WindowRequest<P>>,
    ) -> Entity<ViewWindows<P>> {
        let windows = app.new_entity(|_| ViewWindows {
            awaiting: Vec::new(),
            given: Vec::new(),
            shown: Vec::new(),
            activation_tokens: HashMap::new(),
        });
        take_requests(app, windows.downgrade(), requests);
        let reports = Rc::new(ViewReports {
            windows: windows.downgrade(),
        });
        WidgetsBinding::instance(app).add_observer(app, reports);
        windows
    }

    /// Whether any window is shown.
    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// Whether `program` has a window shown.
    pub fn has_windows(&self, program: &P) -> bool {
        self.shown.iter().any(|window| window.program == *program)
    }

    /// The window shown that has the focus, or else the one that arrived last; none while none is
    /// shown.
    pub fn front_window(&self) -> Option<WindowRef> {
        front_of(self.shown.iter()).map(|window| Rc::clone(&window.window))
    }

    /// Brings the front window of the program `which` picks forward, its focused one or else the
    /// one that arrived last, as the system brings a window forward for the focus, which on
    /// Android brings its task forward; nothing where it has none.
    pub fn bring_forward(&self, which: impl Fn(&P) -> bool) {
        let windows = self.shown.iter().filter(|window| which(&window.program));
        if let Some(window) = front_of(windows) {
            request_focus(window);
        }
    }

    /// Brings the program running as `program` forward for a launch that asked for it, as a pin of
    /// it does: its front window, which on Linux takes the focus with the launch's
    /// `activation_token`; where it has no window yet, its next window shows with the token.
    pub fn bring_forward_for_launch(
        &mut self,
        program: &str,
        activation_token: Option<String>,
        cx: &mut Context<ViewWindows<P>>,
    ) {
        let windows = self
            .shown
            .iter()
            .filter(|window| window.program.name() == program);
        let front = front_of(windows);
        match (front, activation_token) {
            (Some(window), Some(token)) if activate(cx, &window.window, &token) => {}
            (Some(window), _) => request_focus(window),
            (None, Some(token)) => {
                self.activation_tokens.insert(program.to_owned(), token);
            }
            (None, None) => {}
        }
    }

    /// Opens the window the platform gave as `view`, as one it restored, for a program about to
    /// start with it, whose name and icon `face` shows on it: the window gets its canvas, and the
    /// queue the program's calls on it wait in until the program runs. Answers what the program
    /// starts with, or why there is nothing to start with, the window then closed.
    pub fn open_given(
        &mut self,
        view: ViewRef,
        face: &ProgramFace,
        cx: &mut Context<ViewWindows<P>>,
    ) -> Result<OpenedView, &'static str> {
        let (window, canvas) = match window_and_canvas(&view, WindowBackground::Solid, cx) {
            Ok(found) => found,
            Err(reason) => {
                close_window_of(&view);
                return Err(reason);
            }
        };
        window.set_title(&face.name);
        window.set_icon(face.icon.as_ref());
        let (calls, arriving) = view_call_queue();
        let standing = WindowStanding::opened(true, true, &*window);
        let metrics = view.metrics();
        let opened = OpenedView {
            canvas: Arc::clone(&canvas),
            calls,
            metrics: metrics_of(&metrics),
            screen: metrics.screen.map(|screen| screen.0),
            presence: standing.presence(),
            frame: rect_of(window.frame()),
            size_limits: window.size_limits().is_some(),
            maximized: window.is_maximized(),
            title_bar_double_click_action: title_bar_action_of(
                window.title_bar_double_click_action(),
            ),
        };
        self.given.push(GivenWindow {
            view,
            window,
            canvas,
            calls: arriving,
            standing,
            metrics,
        });
        Ok(opened)
    }

    /// The program `program` started with the window the platform gave as `view`, which it has as
    /// `program_view`: the window shows it from now on. Where the window went meanwhile, the
    /// program hears that its view did.
    pub fn given_started(
        &mut self,
        view: ViewId,
        program: P,
        program_view: P::View,
        cx: &mut Context<ViewWindows<P>>,
    ) {
        let Some(index) = self.given.iter().position(|given| given.view.id() == view) else {
            program.remove_view(&program_view);
            return;
        };
        let given = self.given.remove(index);
        tracing::info!(
            program = program.name(),
            view = view.0,
            "a window the platform gave shows the program it started"
        );
        self.show_window(
            None,
            program,
            program_view,
            given.view,
            given.window,
            given.canvas,
            given.standing,
            given.metrics,
            given.calls,
            cx,
        );
    }

    /// No program started with the window the platform gave as `view`: it closes.
    pub fn given_not_started(&mut self, view: ViewId) {
        if let Some(index) = self.given.iter().position(|given| given.view.id() == view) {
            close_window_of(&self.given.remove(index).view);
        }
    }

    /// Waft's view showing the first view `which` picks, with its physical pixels per logical
    /// one; none where no window shows such a view.
    pub fn waft_view_of(
        &self,
        app: &App,
        which: impl Fn(&P, &P::View) -> bool,
    ) -> Option<(ViewId, f64)> {
        self.shown.iter().find_map(|window| {
            which(&window.program, &window.view).then(|| {
                let shown = window.shown.read(app);
                (shown.waft_view(), shown.device_pixel_ratio())
            })
        })
    }

    /// Whether `view` is a program's window here: one a program asked for, waiting for its view or
    /// shown, or one the platform gave, its program starting.
    pub fn knows(&self, view: &ViewRef) -> bool {
        let id = view.id();
        let asked = view.request_id();
        self.shown.iter().any(|window| window.waft_view.id() == id)
            || self.given.iter().any(|given| given.view.id() == id)
            || asked
                .is_some_and(|asked| self.awaiting.iter().any(|awaiting| awaiting.asked == asked))
    }

    /// The program's view shown in the window of `view`, while there is one.
    pub(crate) fn shown_in(&self, view: ViewId) -> Option<&ShownWindow<P>> {
        self.shown
            .iter()
            .find(|window| window.waft_view.id() == view)
    }

    /// Something of the window of `view` changed, as its content heard: it was minimized or
    /// restored, became occluded or shown, or moved. How it stands is read again.
    pub fn window_changed(&mut self, view: ViewId, cx: &mut Context<ViewWindows<P>>) {
        if let Some(given) = self.given.iter_mut().find(|given| given.view.id() == view) {
            given.standing.read_window(&*given.window);
        } else if let Some(window) = self
            .shown
            .iter_mut()
            .find(|window| window.waft_view.id() == view)
        {
            window.window_changed(cx);
        }
    }

    /// The user asked to close the window of `view`: a program's window passes it on to the
    /// program, which decides; one whose program is still starting closes. Answers whether it was
    /// a program's window.
    pub fn close_requested(&self, view: ViewId, app: &App) -> bool {
        if let Some(window) = self.shown_in(view) {
            window.shown.read(app).view().close_requested();
            return true;
        }
        let given = self.given.iter().find(|given| given.view.id() == view);
        if let Some(given) = given {
            given.window.close();
        }
        given.is_some()
    }

    /// Asks Waft for the window `asked` wants; a request Waft refuses at once fails.
    fn ask_for_window(&mut self, asked: WindowRequest<P>, cx: &mut Context<ViewWindows<P>>) {
        let WindowRequest {
            program,
            request,
            face,
        } = asked;
        if has_ended(&program) {
            tracing::info!(program = program.name(), "a request outlived its program");
            return;
        }
        let activation_token = self.activation_tokens.remove(program.name());
        let config = window_config_of(&request.config, &face, activation_token);
        let restorable = RestorableWindow {
            program: program.name().to_owned(),
            role: self.role_of_next(&program),
        };
        let initial_data = Some(restorable.initial_data());
        let (visible, activating) = (config.visible, config.activating);
        let asked = match cx.platform().request_view(config, initial_data) {
            Ok(asked) => asked,
            Err(error) => {
                tracing::info!(program = program.name(), %error, "no window");
                program.view_request_failed(request.id, program_error_of(error));
                return;
            }
        };
        self.awaiting.push(AwaitingWindow {
            asked,
            program,
            request: request.id,
            background: request.config.background,
            visible,
            activating,
        });
    }

    /// Waft added `view`: a window a program asked for is shown for it, or closed unseen where
    /// the program ended meanwhile; any other view is not the windows' to show.
    fn view_added(&mut self, view: &ViewRef, cx: &mut Context<ViewWindows<P>>) {
        let Some(index) = self
            .awaiting
            .iter()
            .position(|awaiting| Some(awaiting.asked) == view.request_id())
        else {
            return;
        };
        let awaiting = self.awaiting.remove(index);
        if has_ended(&awaiting.program) {
            close_window_of(view);
            return;
        }
        self.show(awaiting, Rc::clone(view), cx);
    }

    /// Waft made no window for the request `asked`: the program hears why.
    fn view_request_failed(&mut self, asked: waft::ui::RequestId, error: &ViewError) {
        if let Some(index) = self
            .awaiting
            .iter()
            .position(|awaiting| awaiting.asked == asked)
        {
            self.awaiting
                .remove(index)
                .fail(program_error_of(error.clone()));
        }
    }

    /// What the next window `program` asks for is: its first while it has none shown or coming,
    /// else an extra one.
    fn role_of_next(&self, program: &P) -> WindowRole {
        let coming = self
            .awaiting
            .iter()
            .any(|awaiting| awaiting.program == *program);
        if coming || self.has_windows(program) {
            WindowRole::Extra
        } else {
            WindowRole::First
        }
    }

    /// Adds the program's view, shown in the window of `view`, that `awaiting` asked for.
    fn show(
        &mut self,
        awaiting: AwaitingWindow<P>,
        view: ViewRef,
        cx: &mut Context<ViewWindows<P>>,
    ) {
        let (window, canvas) = match window_and_canvas(&view, awaiting.background, cx) {
            Ok(found) => found,
            Err(reason) => {
                close_window_of(&view);
                awaiting.fail(ProgramViewError::Failed(reason.to_owned()));
                return;
            }
        };
        let (calls, arriving) = view_call_queue();
        let standing = WindowStanding::opened(awaiting.visible, awaiting.activating, &*window);
        let metrics = view.metrics();
        let opened = OpenedView {
            canvas: Arc::clone(&canvas),
            calls,
            metrics: metrics_of(&metrics),
            screen: metrics.screen.map(|screen| screen.0),
            presence: standing.presence(),
            frame: rect_of(window.frame()),
            size_limits: window.size_limits().is_some(),
            maximized: window.is_maximized(),
            title_bar_double_click_action: title_bar_action_of(
                window.title_bar_double_click_action(),
            ),
        };
        let program = awaiting.program;
        let program_view = program.add_view(awaiting.request, opened);
        tracing::info!(
            program = program.name(),
            request = awaiting.request,
            width = metrics.physical_size[0],
            height = metrics.physical_size[1],
            scale = metrics.device_pixel_ratio,
            "a window was made for the view"
        );
        self.show_window(
            Some(awaiting.request),
            program,
            program_view,
            view,
            window,
            canvas,
            standing,
            metrics,
            arriving,
            cx,
        );
    }

    /// Shows `program_view` of `program` in `window`, `view`'s: its canvas
    /// drawn there, its calls arriving on `calls` carried out from now on, the window standing as
    /// `standing` says and the view measuring `metrics` as the program was told; closed once the
    /// program ends.
    #[expect(
        clippy::too_many_arguments,
        reason = "everything a window shown starts with, gathered by the two ways it arrives"
    )]
    fn show_window(
        &mut self,
        request: Option<RequestId>,
        program: P,
        program_view: P::View,
        view: ViewRef,
        window: WindowRef,
        canvas: Arc<CanvasLayer>,
        standing: WindowStanding,
        metrics: ViewMetrics,
        calls: UnboundedReceiver<ViewCall>,
        cx: &mut Context<ViewWindows<P>>,
    ) {
        let shown = ShownView::open(
            cx,
            Rc::new(program_view.clone()) as Rc<dyn ViewLink>,
            canvas,
            Rc::clone(&view),
            metrics,
            standing.presence(),
            calls,
        );
        let window_calls = cx.subscribe(&shown, |windows, shown, call: &WindowCall, cx| {
            windows.carry_out_on_window(&shown, call, cx);
        });
        close_when_ended(cx, &program, view.id());
        let frame = window.frame();
        let maximized = window.is_maximized();
        let title_bar_double_click_action =
            title_bar_action_of(window.title_bar_double_click_action());
        self.shown.push(ShownWindow {
            program,
            request,
            view: program_view,
            shown: shown.clone(),
            waft_view: view,
            window,
            standing,
            frame,
            maximized,
            title_bar_double_click_action,
            _window_calls: window_calls,
        });
        cx.notify();
    }

    /// Carries out the program's `call` on the window of `shown`.
    fn carry_out_on_window(
        &mut self,
        shown: &Entity<ShownView>,
        call: &WindowCall,
        cx: &mut Context<ViewWindows<P>>,
    ) {
        if let Some(window) = self.shown.iter_mut().find(|window| window.shown == *shown) {
            tracing::trace!(request = ?window.request, ?call, "a call on a window");
            window.carry_out(call, cx);
        }
    }

    /// Waft removed `view`, its window gone: a program's view in it is taken away, the program
    /// hearing so, and a window the platform gave is let go of.
    fn view_removed(&mut self, view: ViewId, cx: &mut Context<ViewWindows<P>>) {
        self.given.retain(|given| given.view.id() != view);
        let Some(index) = self
            .shown
            .iter()
            .position(|window| window.waft_view.id() == view)
        else {
            return;
        };
        let window = self.shown.remove(index);
        window.shown.read(cx).gone();
        window.program.remove_view(&window.view);
        cx.notify();
    }

    /// The app's lifecycle changed, which may hide or show every window at once while their
    /// content builds nothing: each window is read again.
    fn lifecycle_changed(&mut self, cx: &mut Context<ViewWindows<P>>) {
        for given in &mut self.given {
            given.standing.read_window(&*given.window);
        }
        for window in &mut self.shown {
            window.window_changed(cx);
        }
    }

    /// The user's Back in `view`: to its program, where the program goes back within itself, and
    /// otherwise the window goes to the background, its program running on. Answers whether a
    /// program's window it was.
    fn go_back(&mut self, view: ViewId, cx: &mut Context<ViewWindows<P>>) -> bool {
        let Some(window) = self
            .shown
            .iter_mut()
            .find(|window| window.waft_view.id() == view)
        else {
            return false;
        };
        let handled = {
            let shown = window.shown.read(cx);
            if shown.handles_back() {
                shown.pop_route();
            }
            shown.handles_back()
        };
        if !handled {
            tracing::info!(
                program = window.program.name(),
                "Back at the program's first screen: its window goes to the background"
            );
            window.window.set_minimized(true);
            window.window_changed(cx);
        }
        true
    }

    /// Waft says the window of `event`'s view gained or lost the input focus.
    fn view_focus_changed(&mut self, event: ViewFocusEvent, cx: &mut Context<ViewWindows<P>>) {
        let focused = event.state == ViewFocusState::Focused;
        for window in &mut self.shown {
            if window.shown.read(cx).waft_view() == event.view_id {
                window.focus_changed(focused, cx);
            }
        }
    }

    /// Something of the views' metrics changed: each view reads its own again, and its window's
    /// frame.
    fn metrics_changed(&mut self, cx: &mut Context<ViewWindows<P>>) {
        for window in &mut self.shown {
            let metrics = window.waft_view.metrics();
            window
                .shown
                .update(cx, |shown, _| shown.set_metrics(metrics));
            window.read_window_again();
        }
    }
}

impl<P: ProgramLink> ShownWindow<P> {
    /// Carries out what the program asked of the window.
    fn carry_out(&mut self, call: &WindowCall, app: &mut App) {
        match call {
            WindowCall::SetFrame(frame, animate) => {
                self.window.set_frame(waft_rect(*frame), *animate);
                self.read_window_again();
            }
            WindowCall::SetTitle(title) => self.window.set_title(title),
            WindowCall::Show => {
                self.window.show();
                self.restand(app, |standing| standing.hidden = false);
            }
            WindowCall::Hide => {
                self.window.hide();
                self.restand(app, |standing| standing.hidden = true);
            }
            WindowCall::Close => self.window.close(),
            WindowCall::SetMinSize(size) => {
                if let Some(limits) = self.window.size_limits() {
                    limits.set_min_size(size.map(waft_size));
                }
            }
            WindowCall::SetMaxSize(size) => {
                if let Some(limits) = self.window.size_limits() {
                    limits.set_max_size(size.map(waft_size));
                }
            }
            WindowCall::Drag => self.window.drag(),
            WindowCall::SetMaximized(maximized) => {
                self.window.set_maximized(*maximized);
                self.window_changed(app);
            }
            WindowCall::SetMinimized(minimized) => {
                self.window.set_minimized(*minimized);
                self.window_changed(app);
            }
        }
    }

    /// Waft says the window was minimized or restored, became occluded or shown, or moved: the
    /// program hears how the view stands, where the window is and whether it is maximized, each
    /// if it changed.
    fn window_changed(&mut self, app: &mut App) {
        let window = Rc::clone(&self.window);
        self.restand(app, |standing| standing.read_window(&*window));
        self.read_window_again();
    }

    /// Waft says the window gained or lost the input focus: the program hears how the view
    /// stands, and what a double-click on a title bar does, if either changed.
    fn focus_changed(&mut self, focused: bool, app: &mut App) {
        self.restand(app, |standing| standing.focused = focused);
        self.read_title_bar_double_click_action_again();
    }

    /// Reads the window's frame again, and whether it is maximized where the frame changed, and
    /// tells the program what changed.
    fn read_window_again(&mut self) {
        let frame = self.window.frame();
        if frame == self.frame {
            return;
        }
        self.frame = frame;
        self.view.window_frame_changed(rect_of(frame));
        let maximized = self.window.is_maximized();
        if maximized != self.maximized {
            self.maximized = maximized;
            self.view.window_maximized_changed(maximized);
        }
    }

    fn read_title_bar_double_click_action_again(&mut self) {
        let action = title_bar_action_of(self.window.title_bar_double_click_action());
        if action != self.title_bar_double_click_action {
            self.title_bar_double_click_action = action;
            self.view.title_bar_double_click_action_changed(action);
        }
    }

    /// Changes how the window stands, and the view's presence with it.
    fn restand(&mut self, app: &mut App, change: impl FnOnce(&mut WindowStanding)) {
        change(&mut self.standing);
        let presence = self.standing.presence();
        self.shown
            .update(app, |shown, _| shown.set_presence(presence));
    }
}

impl<P: ProgramLink> AwaitingWindow<P> {
    /// The program hears that its request got no window.
    fn fail(self, error: ProgramViewError) {
        tracing::info!(
            program = self.program.name(),
            request = self.request,
            ?error,
            "no window"
        );
        self.program.view_request_failed(self.request, error);
    }
}

/// The window `view` is in, and the composition layer of its canvas, which the window's frames
/// place; why not, where Waft gave neither.
fn window_and_canvas(
    view: &ViewRef,
    background: WindowBackground,
    app: &App,
) -> Result<(WindowRef, Arc<CanvasLayer>), &'static str> {
    let window = view.window().ok_or("Waft gave the view no window")?;
    let compositor = app
        .platform()
        .compositor()
        .ok_or("this system shows no composition layers")?;
    Ok((
        window,
        Arc::new(CanvasLayer::make(&*compositor, background)),
    ))
}

fn has_ended(program: &impl ProgramLink) -> bool {
    program.ended().now_or_never().is_some()
}

/// Of `windows`, the one that has the focus, or else the one that arrived last.
fn front_of<'a, P: ProgramLink + 'a>(
    windows: impl DoubleEndedIterator<Item = &'a ShownWindow<P>> + Clone,
) -> Option<&'a ShownWindow<P>> {
    let mut focused = windows.clone().filter(|window| window.standing.focused);
    focused.next().or_else(|| windows.last())
}

/// Asks for the focus for `window`'s view, as the system gives it to a window brought forward.
fn request_focus<P: ProgramLink>(window: &ShownWindow<P>) {
    window
        .waft_view
        .host()
        .request_focus_change(ViewFocusState::Focused, ViewFocusDirection::Undefined);
}

/// Brings `window` forward with the activation token `token`, through Waft's desktop host on Linux;
/// answers whether it could, which it cannot elsewhere.
#[cfg(target_os = "linux")]
fn activate(app: &App, window: &WindowRef, token: &str) -> bool {
    let platform = app.platform();
    let linux = platform
        .downcast_ref::<waft_embedder_winit::WinitPlatform>()
        .and_then(|winit| winit.linux());
    if let Some(linux) = linux {
        linux.activate(window, token);
    }
    linux.is_some()
}

#[cfg(not(target_os = "linux"))]
fn activate(_app: &App, _window: &WindowRef, _token: &str) -> bool {
    false
}

fn program_error_of(error: ViewError) -> ProgramViewError {
    match error {
        ViewError::Unsupported => ProgramViewError::Unsupported,
        ViewError::Failed(reason) => ProgramViewError::Failed(reason),
    }
}

/// One task takes every request the programs make, on the main thread, for as long as the
/// windows are there to take them.
fn take_requests<P: ProgramLink>(
    app: &mut App,
    windows: WeakEntity<ViewWindows<P>>,
    mut requests: UnboundedReceiver<WindowRequest<P>>,
) {
    app.spawn(async move |cx| {
        while let Some(request) = requests.next().await {
            let Some(windows) = windows.upgrade() else {
                break;
            };
            cx.update(|app| windows.update(app, |windows, cx| windows.ask_for_window(request, cx)));
        }
    });
}

/// Closes the window `view` is in, where it is in one.
fn close_window_of(view: &ViewRef) {
    if let Some(window) = view.window() {
        window.close();
    }
}

/// Closes the window showing `view` once `program` has ended, if it is still shown.
fn close_when_ended<P: ProgramLink>(cx: &mut Context<ViewWindows<P>>, program: &P, view: ViewId) {
    let ended = program.ended();
    let windows = cx.weak_entity();
    cx.spawn(async move |cx| {
        ended.await;
        cx.update(|app| {
            let Some(windows) = windows.upgrade() else {
                return;
            };
            let windows = windows.read(app);
            let mut shown = windows.shown.iter();
            if let Some(shown) = shown.find(|shown| shown.waft_view.id() == view) {
                shown.window.close();
            }
        });
    });
}

/// What Waft reports of the views, passed on to the windows.
struct ViewReports<P: ProgramLink> {
    windows: WeakEntity<ViewWindows<P>>,
}

impl<P: ProgramLink> ViewReports<P> {
    fn update(
        &self,
        app: &mut App,
        f: impl FnOnce(&mut ViewWindows<P>, &mut Context<ViewWindows<P>>),
    ) {
        if let Some(windows) = self.windows.upgrade() {
            windows.update(app, f);
        }
    }
}

impl<P: ProgramLink> WidgetsBindingObserver for ViewReports<P> {
    fn did_add_view(&self, app: &mut App, view: &ViewRef) {
        self.update(app, |windows, cx| windows.view_added(view, cx));
    }

    fn did_remove_view(&self, app: &mut App, view: ViewId) {
        self.update(app, |windows, cx| windows.view_removed(view, cx));
    }

    fn did_change_app_lifecycle_state(&self, app: &mut App, _state: AppLifecycleState) {
        self.update(app, ViewWindows::lifecycle_changed);
    }

    fn did_fail_view_request(
        &self,
        app: &mut App,
        request: waft::ui::RequestId,
        error: &ViewError,
    ) {
        self.update(app, |windows, _| {
            windows.view_request_failed(request, error)
        });
    }

    fn did_change_metrics(&self, app: &mut App) {
        self.update(app, ViewWindows::metrics_changed);
    }

    fn did_change_view_focus(&self, app: &mut App, event: ViewFocusEvent) {
        self.update(app, |windows, cx| windows.view_focus_changed(event, cx));
    }

    fn did_pop_route(&self, app: &mut App, view: ViewId) -> bool {
        let Some(windows) = self.windows.upgrade() else {
            return false;
        };
        windows.update(app, |windows, cx| windows.go_back(view, cx))
    }
}
