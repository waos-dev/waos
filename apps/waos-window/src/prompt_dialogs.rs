//! WaOS's prompts for the program, each in a dialog window of its own in the program's app
//! (decision 9 of `docs/proposals/2026-10-29-macos-apps.md`): a window of the window process's,
//! asked for hidden and shown once it arrived, centred on the screen of the program's front window,
//! or on the main screen while the program has none (`dialog_frame`), whose canvas shows the layer
//! context WaOS draws the prompt in (`hosted_layers`), so the window process draws nothing of it
//! (decision 5). Showing it leaves the app where it is: an app in the background asks for the
//! user's attention instead (`os::Attention`), which bounces its Dock icon. What Waft's host
//! reports of the dialog's view goes to WaOS as the prompt's reports: its metrics as it is shown
//! and each time they change, whether it is in sight, and its focus; its canvas sends its pointers
//! and keys (`prompt_canvas`). What WaOS's view asks of it, a cursor or the keyboard's focus, goes
//! to the dialog's view's host, as Waft's own views ask theirs. The dialog's window arrives as every
//! view does, and is built by whoever builds the process's views from the dialogs' content for it
//! (`PromptDialogs::content_of`), which reports each change of the window back here.
//!
//! The invariant a later editor will break: a dialog is shown from WaOS's `ShowPrompt` until its
//! `HidePrompt`, or until it goes otherwise, when the user closes it or no window could be made for
//! it, which WaOS hears as `Closed`, once; a dialog WaOS hid is gone from here before its window
//! closes, so its closing reports nothing, and one hidden before its window arrived closes it
//! unseen as it arrives.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use waft::ui::{
    CompositionLayer, PlatformRef, RequestId, ViewError, ViewFocusEvent, ViewId, ViewMetrics,
    ViewRef,
};
use waft::{
    App, Context, Entity, IntoWidget, Listener, WeakEntity, WidgetRef, WidgetsBinding,
    WidgetsBindingObserver, WindowConfig, WindowRef,
};
use waos_program_window::{ViewWindows, WindowChanges};
use waos_window_messages::{FromWindow, HostedLayers, PromptCall, PromptId, PromptReport};

use crate::connection::Connection;
use crate::dialog_frame::{centred_frame, screen_for};
use crate::hosted_layers::show_hosted_layers;
use crate::os::{Attention, LayerHost};
use crate::program_link::WaosProgram;
use crate::prompt_canvas::PromptCanvas;

/// The dialogs of WaOS's prompts for the program.
pub(crate) struct PromptDialogs {
    /// The program's windows, the front one of which says the screen a dialog is centred on.
    view_windows: Entity<ViewWindows<WaosProgram>>,
    /// Waft's platform, whose macOS host shows WaOS's layers on a dialog's canvas.
    platform: PlatformRef,
    dialogs: Vec<Dialog>,
    /// The windows of dialogs hidden before their windows arrived, which close as they do.
    closing_on_arrival: Vec<RequestId>,
}

/// One prompt's dialog.
struct Dialog {
    link: Rc<DialogLink>,
    /// What Waft answered the request for its window with, which the window's view arrives
    /// carrying.
    asked: RequestId,
    /// Its window's view, from its arrival.
    view: Option<ViewRef>,
    /// The host of WaOS's context, which the canvas's layer shows.
    _host: LayerHost,
    /// Whether it is shown, which it is from its window's arrival on.
    shown: bool,
    /// The user's attention, asked for as it was shown while the app was in the background.
    _attention: Option<Attention>,
    /// What WaOS was last told of the dialog's view.
    metrics: Option<ViewMetrics>,
    in_sight: bool,
}

/// What a dialog's canvas shows, and where it reports what its pointers and keys do.
pub(crate) struct DialogLink {
    prompt: PromptId,
    connection: Rc<Connection>,
    /// The layer the canvas shows, whose content is WaOS's context.
    layer: Arc<dyn CompositionLayer>,
    /// Physical pixels per logical one on the dialog's view, as WaOS was last told.
    ratio: Cell<f64>,
}

impl PromptDialogs {
    /// The dialogs, centred by the windows of `view_windows`, which follow what Waft reports of
    /// the dialogs' views.
    pub(crate) fn new(
        app: &mut App,
        view_windows: Entity<ViewWindows<WaosProgram>>,
    ) -> Entity<PromptDialogs> {
        let platform = app.platform();
        let dialogs = app.new_entity(|_| PromptDialogs {
            view_windows,
            platform,
            dialogs: Vec::new(),
            closing_on_arrival: Vec::new(),
        });
        let reports = Rc::new(DialogViewReports {
            dialogs: dialogs.downgrade(),
        });
        WidgetsBinding::instance(app).add_observer(app, reports);
        dialogs
    }

    /// What goes in the window of `view`, where it is a dialog's: its canvas, reporting each
    /// change of the window to the dialogs; none for any other view.
    pub(crate) fn content_of(
        dialogs: &Entity<PromptDialogs>,
        app: &App,
        view: &ViewRef,
    ) -> Option<WidgetRef> {
        let dialog = dialogs.read(app).shown_in(view.id())?;
        let prompt = dialog.link.prompt;
        let canvas = PromptCanvas::new(Rc::clone(&dialog.link)).into_widget();
        let dialogs = dialogs.clone();
        let changed = Listener::new(move |app| {
            dialogs.update(app, |dialogs, _| dialogs.window_changed(prompt));
        });
        Some(WindowChanges::new(changed, canvas).into_widget())
    }

    /// The user asked to close the window of `view`: where it is a dialog's, the dialog goes,
    /// unanswered. Answers whether it was.
    pub(crate) fn close_requested(
        &mut self,
        view: ViewId,
        cx: &mut Context<PromptDialogs>,
    ) -> bool {
        let index = self
            .dialogs
            .iter()
            .position(|dialog| dialog.view.as_ref().is_some_and(|own| own.id() == view));
        if let Some(index) = index {
            self.close(index, cx);
        }
        index.is_some()
    }

    /// Shows the prompt `prompt` in a dialog `size` logical points large, showing WaOS's context
    /// `layers`, its reports going through `connection`: its window is asked for now, hidden, and
    /// shown once it arrives.
    pub(crate) fn show(
        &mut self,
        prompt: PromptId,
        size: [f64; 2],
        layers: HostedLayers,
        connection: Rc<Connection>,
        cx: &mut Context<PromptDialogs>,
    ) {
        let Some(compositor) = self.platform.compositor() else {
            tracing::warn!(prompt, "Waft's platform shows no composition layers");
            connection.send(prompt_report(prompt, PromptReport::Closed));
            return;
        };
        let layer = compositor.make_layer(false);
        let host = show_hosted_layers(&self.platform, &layer, layers.context_id);
        let asked = match cx
            .platform()
            .request_view(dialog_config(connection.name(), size), None)
        {
            Ok(asked) => asked,
            Err(error) => {
                tracing::warn!(prompt, %error, "no window for a prompt's dialog");
                connection.send(prompt_report(prompt, PromptReport::Closed));
                return;
            }
        };
        let link = DialogLink {
            prompt,
            connection,
            layer,
            ratio: Cell::new(1.0),
        };
        self.dialogs.push(Dialog {
            link: Rc::new(link),
            asked,
            view: None,
            _host: host,
            shown: false,
            _attention: None,
            metrics: None,
            in_sight: true,
        });
        cx.notify();
    }

    /// Carries out on the dialog of `prompt` what WaOS's view asked of it, through the dialog's
    /// view's host.
    pub(crate) fn call(&self, prompt: PromptId, call: PromptCall) {
        let Some(view) = self.find(prompt).and_then(|dialog| dialog.view.clone()) else {
            tracing::debug!(prompt, ?call, "a call on a dialog not shown");
            return;
        };
        match call {
            // One mouse, so the device picks nothing.
            PromptCall::SetCursor(kind) => view.host().activate_system_cursor(0, kind),
            PromptCall::ChangeFocus { state, direction } => {
                view.host().request_focus_change(state, direction);
            }
        }
    }

    /// Takes the dialog of `prompt` away, as WaOS asked.
    pub(crate) fn hide(&mut self, prompt: PromptId, cx: &mut Context<PromptDialogs>) {
        if let Some(index) = self.index_of(prompt) {
            let dialog = self.dialogs.remove(index);
            dialog.close_window(&mut self.closing_on_arrival);
            cx.notify();
        }
    }

    /// Waft added `view`: a dialog's window is shown from now on; one whose dialog was hidden
    /// meanwhile closes unseen.
    fn view_added(&mut self, view: &ViewRef, cx: &mut Context<PromptDialogs>) {
        let Some(asked) = view.request_id() else {
            return;
        };
        if let Some(index) = self.closing_on_arrival.iter().position(|id| *id == asked) {
            self.closing_on_arrival.remove(index);
            close_window_of(view);
            return;
        }
        let Some(index) = self.dialogs.iter().position(|dialog| dialog.asked == asked) else {
            return;
        };
        self.dialogs[index].view = Some(Rc::clone(view));
        self.open(index, view, cx);
        cx.notify();
    }

    /// Waft removed `view`: where it was a dialog's, the dialog went, and WaOS hears it closed.
    fn view_removed(&mut self, view: ViewId, cx: &mut Context<PromptDialogs>) {
        let index = self
            .dialogs
            .iter()
            .position(|dialog| dialog.view.as_ref().is_some_and(|own| own.id() == view));
        if let Some(index) = index {
            let dialog = self.dialogs.remove(index);
            dialog.link.report(PromptReport::Closed);
            cx.notify();
        }
    }

    /// Waft made no window for `asked`: where it was a dialog's, WaOS hears it closed.
    fn view_request_failed(
        &mut self,
        asked: RequestId,
        error: &ViewError,
        cx: &mut Context<PromptDialogs>,
    ) {
        if let Some(index) = self.dialogs.iter().position(|dialog| dialog.asked == asked) {
            tracing::warn!(prompt = self.dialogs[index].link.prompt, %error, "no window for a prompt's dialog");
            self.close(index, cx);
        }
    }

    /// The window of the dialog of `prompt` changed, as its content heard: WaOS hears whether it
    /// is in sight.
    fn window_changed(&mut self, prompt: PromptId) {
        if let Some(index) = self.index_of(prompt) {
            self.dialogs[index].tell_in_sight();
        }
    }

    /// The window of the dialog at `index` arrived as `view`'s: it is centred and shown, the
    /// user's attention asked for while the app is in the background, and WaOS hears it is shown.
    fn open(&mut self, index: usize, view: &ViewRef, cx: &mut Context<PromptDialogs>) {
        let Some(window) = view.window() else {
            tracing::warn!("Waft gave a prompt's dialog no window");
            self.close(index, cx);
            return;
        };
        self.centre(&window, cx);
        window.show();
        let dialog = &mut self.dialogs[index];
        dialog.shown = true;
        dialog._attention = Attention::ask();
        let metrics = view.metrics();
        dialog.link.ratio.set(metrics.device_pixel_ratio);
        dialog.metrics = Some(metrics);
        tracing::info!(
            prompt = dialog.link.prompt,
            width = metrics.physical_size[0],
            height = metrics.physical_size[1],
            "a prompt's dialog is shown in the program's app"
        );
        dialog.link.report(PromptReport::Shown(metrics));
    }

    /// Moves `window` to the middle of the screen of the program's front window, or of the main
    /// screen while the program has none; leaves it where the host put it where there are no
    /// screens to read.
    fn centre(&self, window: &WindowRef, app: &App) {
        let front_screen = self
            .view_windows
            .read(app)
            .front_window()
            .and_then(|front| front.view().metrics().screen);
        let screens = self
            .platform
            .screens()
            .map(|screens| screens.screens())
            .unwrap_or_default();
        if let Some(screen) = screen_for(&screens, front_screen) {
            window.set_frame(centred_frame(window.frame().size(), screen), None);
        }
    }

    /// The dialog at `index` went otherwise than as WaOS asked: WaOS hears it closed.
    fn close(&mut self, index: usize, cx: &mut Context<PromptDialogs>) {
        let dialog = self.dialogs.remove(index);
        dialog.link.report(PromptReport::Closed);
        dialog.close_window(&mut self.closing_on_arrival);
        cx.notify();
    }

    /// The dialog whose window is `view`'s.
    fn shown_in(&self, view: ViewId) -> Option<&Dialog> {
        self.dialogs
            .iter()
            .find(|dialog| dialog.view.as_ref().is_some_and(|own| own.id() == view))
    }

    /// Something of the views' metrics changed: WaOS hears each shown dialog's that changed.
    fn metrics_changed(&mut self) {
        for dialog in &mut self.dialogs {
            let Some(view) = dialog.view.clone() else {
                continue;
            };
            let metrics = view.metrics();
            if dialog.shown && dialog.metrics != Some(metrics) {
                dialog.link.ratio.set(metrics.device_pixel_ratio);
                dialog.metrics = Some(metrics);
                dialog.link.report(PromptReport::MetricsChanged(metrics));
            }
        }
    }

    /// Waft says `event`'s view gained or lost the input focus: WaOS hears it of a dialog's.
    fn view_focus_changed(&self, event: ViewFocusEvent) {
        let dialog = self.dialogs.iter().find(|dialog| {
            dialog
                .view
                .as_ref()
                .is_some_and(|view| view.id() == event.view_id)
        });
        if let Some(dialog) = dialog {
            dialog.link.report(PromptReport::Focus(event));
        }
    }

    fn find(&self, prompt: PromptId) -> Option<&Dialog> {
        self.dialogs
            .iter()
            .find(|dialog| dialog.link.prompt == prompt)
    }

    fn index_of(&self, prompt: PromptId) -> Option<usize> {
        self.dialogs
            .iter()
            .position(|dialog| dialog.link.prompt == prompt)
    }
}

impl Dialog {
    /// Closes the dialog's window, or, while it is coming, notes in `closing_on_arrival` that it
    /// closes as it arrives.
    fn close_window(self, closing_on_arrival: &mut Vec<RequestId>) {
        match &self.view {
            Some(view) => close_window_of(view),
            None => closing_on_arrival.push(self.asked),
        }
    }

    /// WaOS hears whether the dialog is in sight, if that changed: not while it is minimized or
    /// covered.
    fn tell_in_sight(&mut self) {
        let Some(window) = self.view.as_ref().and_then(|view| view.window()) else {
            return;
        };
        let in_sight = !(window.is_minimized() || window.is_occluded());
        if in_sight != self.in_sight {
            self.in_sight = in_sight;
            self.link.report(PromptReport::InSight(in_sight));
        }
    }
}

impl DialogLink {
    /// The layer the canvas shows.
    pub(crate) fn layer(&self) -> Arc<dyn CompositionLayer> {
        Arc::clone(&self.layer)
    }

    /// Physical pixels per logical one on the dialog's view.
    pub(crate) fn ratio(&self) -> f64 {
        self.ratio.get()
    }

    pub(crate) fn prompt(&self) -> PromptId {
        self.prompt
    }

    /// Tells WaOS `report` of the dialog.
    pub(crate) fn report(&self, report: PromptReport) {
        self.connection.send(prompt_report(self.prompt, report));
    }
}

fn prompt_report(prompt: PromptId, report: PromptReport) -> FromWindow {
    FromWindow::Prompt { prompt, report }
}

/// A dialog's window, `size` logical points large, its title the program's `name`: of the size
/// WaOS asked for, and hidden until it is centred.
fn dialog_config(name: &str, size: [f64; 2]) -> WindowConfig {
    WindowConfig {
        title: name.to_owned(),
        size,
        resizable: false,
        visible: false,
        ..WindowConfig::default()
    }
}

/// Closes the window `view` is in, where it is in one.
fn close_window_of(view: &ViewRef) {
    if let Some(window) = view.window() {
        window.close();
    }
}

/// What Waft reports of the views, passed on to the dialogs.
struct DialogViewReports {
    dialogs: WeakEntity<PromptDialogs>,
}

impl WidgetsBindingObserver for DialogViewReports {
    fn did_change_metrics(&self, app: &mut App) {
        if let Some(dialogs) = self.dialogs.upgrade() {
            dialogs.update(app, |dialogs, _| dialogs.metrics_changed());
        }
    }

    fn did_change_view_focus(&self, app: &mut App, event: ViewFocusEvent) {
        if let Some(dialogs) = self.dialogs.upgrade() {
            dialogs.update(app, |dialogs, _| dialogs.view_focus_changed(event));
        }
    }

    fn did_add_view(&self, app: &mut App, view: &ViewRef) {
        if let Some(dialogs) = self.dialogs.upgrade() {
            dialogs.update(app, |dialogs, cx| dialogs.view_added(view, cx));
        }
    }

    fn did_remove_view(&self, app: &mut App, view: ViewId) {
        if let Some(dialogs) = self.dialogs.upgrade() {
            dialogs.update(app, |dialogs, cx| dialogs.view_removed(view, cx));
        }
    }

    fn did_fail_view_request(&self, app: &mut App, request: RequestId, error: &ViewError) {
        if let Some(dialogs) = self.dialogs.upgrade() {
            dialogs.update(app, |dialogs, cx| {
                dialogs.view_request_failed(request, error, cx)
            });
        }
    }
}
