//! WaOS's prompts on macOS, each in a dialog window of its own in the program's app and drawn by
//! WaOS (decision 9 of `docs/proposals/2026-10-29-macos-apps.md`): for each prompt WaOS makes a
//! composition layer, places it in a layer context of its own, and asks the program's window
//! process for a dialog showing that context (`logic::program_apps`); once the dialog reports its
//! size and scale, WaOS asks Waft's view embedder for a view drawing into the layer
//! (`ViewEmbedder::add_view`), its initial data naming the prompt ([`dialog_data`]); the view arrives
//! as every view does, and WaOS's one builder builds the prompt's widget in it from that data
//! ([`ShownDialogs::content_of`], `ui::every_window`), and WaOS hands it what
//! the dialog reports: its metrics (`set_view_metrics`, the layer placed again at them), whether it
//! is in sight (`set_view_in_sight`), its pointers (`send_pointer_reports`, each device under a
//! number of WaOS's own, `logic::forwarded_pointers`), its keys (the framework's own entry,
//! `KeyEventManager::handle_key_data`, as Waft's shell hands it its host's keys, so they go where
//! the framework's focus is) and its focus (`send_view_focus_event`). The view's cursor and focus
//! requests go to the dialog ([`DialogHost`]); it takes no text. A prompt takes the getter of its
//! program's app (decision 15 of the proposal): a program whose app does not run has it launched,
//! with no window and in the background, and the prompt waits for it, its dialog shown there once
//! it said hello; a prompt whose app never comes is put away unanswered, as one whose dialog went.
//!
//! The invariant a later editor will break: a prompt's layer, context and view are kept from the
//! moment its app is there until its answer is dropped, when the view is removed, the dialog hidden
//! and the context let go, once; a prompt whose answer is dropped while its app comes is never
//! shown, and a view that arrives for a prompt no longer shown is removed as it arrives; what a
//! dialog reports before its view arrives is kept, and given to the view as it arrives.

use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use waft::ui::{
    CompositionLayer, Compositor, KeyData, PointerReport, RequestId, SystemMouseCursorKind,
    TextInputHost, ViewError, ViewFocusDirection, ViewFocusEvent, ViewFocusState, ViewHost, ViewId,
    ViewMetrics, ViewRef,
};
use waft::{App, Context, Entity, WeakEntity, WidgetRef, WidgetsBinding, WidgetsBindingObserver};
use waft_services::KeyEventManager;
use waos::RunningProgram;
use waos_program_window::{PROMPT_SIZE, PromptPlacement, ShownPrompt};
use waos_window_messages::{HostedLayers, PromptCall, PromptId, PromptReport};

use crate::logic::forwarded_pointers::ForwardedDevices;
use crate::logic::program_apps::{AppReach, CanvasPlace, DialogReport, ReachedApp, WindowProcess};
use crate::os::LayerContext;
use crate::state::program_apps::place_in_context;

/// What the initial data of a prompt's view starts with, the prompt's id following.
const DIALOG_DATA: &str = "waos-prompt-dialog:";

/// The placement of the programs' prompts on macOS.
pub(crate) struct PromptDialogs {
    /// How a prompt reaches its program's app.
    reach: Arc<AppReach>,
    /// What each prompt's layer is made on.
    compositor: Arc<dyn Compositor>,
    shown: Entity<ShownDialogs>,
    /// Where an answer dropped says which dialog to take away.
    hidden: UnboundedSender<PromptId>,
}

/// The prompts shown in dialogs, in the order shown, and those whose app is coming.
pub(crate) struct ShownDialogs {
    dialogs: Vec<Dialog>,
    /// The prompts waiting for their program's app, in the order asked.
    awaiting: Vec<AwaitingApp>,
    last_prompt: PromptId,
}

/// A prompt waiting for its program's app, which it is shown in once the app is there.
struct AwaitingApp {
    prompt: PromptId,
    content: WidgetRef,
    dismissed: Rc<dyn Fn(&mut App)>,
}

/// A prompt shown in a dialog of its program's app.
struct Dialog {
    prompt: PromptId,
    process: Arc<WindowProcess>,
    content: WidgetRef,
    dismissed: Rc<dyn Fn(&mut App)>,
    layer: Arc<dyn CompositionLayer>,
    context: LayerContext,
    /// What Waft answered the request for the view the prompt is drawn in with, asked for once
    /// the dialog said its size.
    asked: Option<RequestId>,
    /// The view the prompt is drawn in, once it arrived.
    view: Option<ViewRef>,
    /// What the dialog reported last, which the view is given as it arrives.
    metrics: Option<ViewMetrics>,
    in_sight: bool,
    focus: Option<ViewFocusEvent>,
    devices: ForwardedDevices,
}

/// What is left to do once a dialog's report is taken, outside the dialogs.
enum Afterwards {
    Nothing,
    /// Hand the key to the framework.
    Key(KeyData),
    /// The prompt was put away unanswered.
    Dismiss(Rc<dyn Fn(&mut App)>),
}

impl PromptDialogs {
    /// The placement, over the programs' apps `reach` reaches, each prompt's layer made on
    /// `compositor`, whose dialogs' reports arrive on `reports`.
    pub(crate) fn new(
        app: &mut App,
        reach: Arc<AppReach>,
        compositor: Arc<dyn Compositor>,
        reports: UnboundedReceiver<DialogReport>,
    ) -> PromptDialogs {
        let shown = app.new_entity(|_| ShownDialogs {
            dialogs: Vec::new(),
            awaiting: Vec::new(),
            last_prompt: 0,
        });
        take_reports(app, &shown, reports);
        let arrivals = Rc::new(DialogViews {
            shown: shown.downgrade(),
        });
        WidgetsBinding::instance(app).add_observer(app, arrivals);
        let (hidden, taken_away) = mpsc::unbounded();
        take_away_when_dropped(app, &shown, taken_away);
        PromptDialogs {
            reach,
            compositor,
            shown,
            hidden,
        }
    }

    /// The prompts shown in dialogs, which their views' content is built from.
    pub(crate) fn shown(&self) -> Entity<ShownDialogs> {
        self.shown.clone()
    }
}

/// The initial data of the view `prompt` is drawn in, which names it.
pub(crate) fn dialog_data(prompt: PromptId) -> String {
    format!("{DIALOG_DATA}{prompt}")
}

/// The prompt `view` is drawn in, by its initial data; none for any other view.
fn prompt_of(view: &ViewRef) -> Option<PromptId> {
    view.initial_data()?.strip_prefix(DIALOG_DATA)?.parse().ok()
}

impl PromptPlacement for PromptDialogs {
    /// In a dialog of the program's app, once the app is there: launched for the prompt when it
    /// does not run.
    fn show(
        &self,
        app: &mut App,
        program: &RunningProgram,
        prompt: WidgetRef,
        dismissed: Rc<dyn Fn(&mut App)>,
    ) -> ShownPrompt {
        let shown = self
            .shown
            .update(app, |shown, _cx| shown.await_app(prompt, dismissed));
        self.show_once_reached(app, program, shown);
        let hidden = self.hidden.clone();
        ShownPrompt::new(move || {
            let _ = hidden.unbounded_send(shown);
        })
    }
}

impl PromptDialogs {
    /// Shows `prompt` in a dialog of `program`'s app once the getter hands the app over, on the
    /// main thread.
    fn show_once_reached(&self, app: &mut App, program: &RunningProgram, prompt: PromptId) {
        let reached = self.reach.app(program);
        let (shown, compositor) = (self.shown.downgrade(), Arc::clone(&self.compositor));
        app.spawn(async move |cx| {
            let reached = reached.await;
            cx.update(|app| {
                let Some(shown) = shown.upgrade() else {
                    return;
                };
                let afterwards = shown.update(app, |shown, cx| {
                    shown.app_reached(prompt, reached, &*compositor, cx)
                });
                afterwards.carry_out(app);
            });
        });
    }
}

impl ShownDialogs {
    /// What goes in `view`, where it is a prompt's: the prompt's widget; none for any other view.
    pub(crate) fn content_of(&self, view: &ViewRef) -> Option<WidgetRef> {
        let prompt = prompt_of(view)?;
        let dialog = self.dialogs.iter().find(|dialog| dialog.prompt == prompt)?;
        dialog
            .view
            .as_ref()
            .is_some_and(|own| own.id() == view.id())
            .then(|| dialog.content.clone())
    }

    /// `content`, a prompt waiting for its program's app; answers the prompt's id.
    fn await_app(&mut self, content: WidgetRef, dismissed: Rc<dyn Fn(&mut App)>) -> PromptId {
        self.last_prompt += 1;
        let prompt = self.last_prompt;
        self.awaiting.push(AwaitingApp {
            prompt,
            content,
            dismissed,
        });
        prompt
    }

    /// The getter handed over what `reached` says for the app of `prompt`: the prompt is shown in a
    /// dialog there, its layer made on `compositor`; or put away unanswered when the app never came.
    /// Nothing for a prompt put away meanwhile.
    fn app_reached(
        &mut self,
        prompt: PromptId,
        reached: ReachedApp,
        compositor: &dyn Compositor,
        cx: &mut Context<ShownDialogs>,
    ) -> Afterwards {
        let Some(index) = self
            .awaiting
            .iter()
            .position(|awaiting| awaiting.prompt == prompt)
        else {
            return Afterwards::Nothing;
        };
        let AwaitingApp {
            content, dismissed, ..
        } = self.awaiting.remove(index);
        match reached {
            Ok(process) => {
                self.show(prompt, process, compositor, content, dismissed, cx);
                Afterwards::Nothing
            }
            Err(why) => {
                tracing::warn!(prompt, why, "a prompt's program's app never came");
                Afterwards::Dismiss(dismissed)
            }
        }
    }

    /// Shows `content`, the prompt `prompt`, in a dialog of `process`'s program's app, its layer
    /// made by `compositor`.
    fn show(
        &mut self,
        prompt: PromptId,
        process: Arc<WindowProcess>,
        compositor: &dyn Compositor,
        content: WidgetRef,
        dismissed: Rc<dyn Fn(&mut App)>,
        cx: &mut Context<ShownDialogs>,
    ) {
        let context = LayerContext::new();
        let layers = HostedLayers {
            context_id: context.id(),
        };
        tracing::info!(
            program = process.program().name(),
            prompt,
            context = layers.context_id,
            "a prompt is shown in a dialog of the program's app"
        );
        process.show_prompt(prompt, PROMPT_SIZE, layers);
        self.dialogs.push(Dialog {
            prompt,
            process,
            content,
            dismissed,
            layer: compositor.make_layer(false),
            context,
            asked: None,
            view: None,
            metrics: None,
            in_sight: true,
            focus: None,
            devices: ForwardedDevices::default(),
        });
        cx.notify();
    }

    /// The answer to `prompt` was dropped: its view is removed, its dialog hidden and its context
    /// let go; one still waiting for its app is never shown.
    fn hide(&mut self, prompt: PromptId, cx: &mut Context<ShownDialogs>) {
        self.awaiting.retain(|awaiting| awaiting.prompt != prompt);
        let Some(index) = self.index_of(prompt) else {
            return;
        };
        let dialog = self.dialogs.remove(index);
        if let Some(view) = &dialog.view {
            remove_view(cx, view.id());
        }
        dialog.process.hide_prompt(prompt);
        cx.notify();
    }

    /// Takes what the dialog of `prompt` reported; answers what is left to do.
    fn reported(
        &mut self,
        prompt: PromptId,
        report: PromptReport,
        cx: &mut Context<ShownDialogs>,
    ) -> Afterwards {
        let Some(index) = self.index_of(prompt) else {
            return Afterwards::Nothing;
        };
        let dialog = &mut self.dialogs[index];
        match report {
            PromptReport::Shown(metrics) => {
                dialog.resized(metrics, cx);
                return self.ask_for_view(index, cx);
            }
            PromptReport::MetricsChanged(metrics) => dialog.resized(metrics, cx),
            PromptReport::InSight(in_sight) => dialog.set_in_sight(in_sight, cx),
            PromptReport::Pointer(pointer) => dialog.pointer(&pointer, cx),
            PromptReport::Focus(event) => dialog.focus(event, cx),
            PromptReport::Key(key) if dialog.view.is_some() => return Afterwards::Key(key),
            PromptReport::Key(_) => {}
            PromptReport::Closed => return Afterwards::Dismiss(Rc::clone(&dialog.dismissed)),
        }
        Afterwards::Nothing
    }

    /// Asks Waft for the view the prompt at `index` is drawn in, at the size its dialog said, its
    /// initial data naming the prompt; it arrives as every view does.
    fn ask_for_view(&mut self, index: usize, cx: &mut Context<ShownDialogs>) -> Afterwards {
        let dialog = &mut self.dialogs[index];
        let (Some(metrics), None) = (dialog.metrics, dialog.asked) else {
            return Afterwards::Nothing;
        };
        let host = Rc::new(DialogHost {
            process: Arc::clone(&dialog.process),
            prompt: dialog.prompt,
        });
        let platform = cx.platform();
        let Some(embedder) = platform.view_embedder() else {
            tracing::warn!(
                "Waft's platform adds no view of the app's own, so a prompt is not shown"
            );
            return Afterwards::Dismiss(Rc::clone(&dialog.dismissed));
        };
        let data = Some(dialog_data(dialog.prompt));
        match embedder.add_view(&dialog.layer, metrics, host, data) {
            Ok(asked) => {
                dialog.asked = Some(asked);
                Afterwards::Nothing
            }
            Err(error) => {
                tracing::warn!(prompt = dialog.prompt, %error, "no view for a prompt");
                Afterwards::Dismiss(Rc::clone(&dialog.dismissed))
            }
        }
    }

    /// Waft added `view`: where it is a prompt's, it is given what the dialog reported while it
    /// was asked for, and shows the prompt from now on; one whose prompt is no longer shown is
    /// removed.
    fn view_added(&mut self, view: &ViewRef, cx: &mut Context<ShownDialogs>) {
        let Some(prompt) = prompt_of(view) else {
            return;
        };
        let Some(index) = self.index_of(prompt) else {
            remove_view(cx, view.id());
            return;
        };
        let dialog = &mut self.dialogs[index];
        dialog.view = Some(Rc::clone(view));
        dialog.give_reported(view, cx);
        cx.notify();
    }

    /// Waft removed `view`: where it was a prompt's, the prompt has no view from now on.
    fn view_removed(&mut self, view: ViewId, cx: &mut Context<ShownDialogs>) {
        let dialog = self
            .dialogs
            .iter_mut()
            .find(|dialog| dialog.view.as_ref().is_some_and(|own| own.id() == view));
        if let Some(dialog) = dialog {
            dialog.view = None;
            cx.notify();
        }
    }

    /// Waft made no view for `asked`: where it was a prompt's, the prompt is put away unanswered.
    fn view_request_failed(&self, asked: RequestId, error: &ViewError) -> Afterwards {
        let Some(dialog) = self
            .dialogs
            .iter()
            .find(|dialog| dialog.asked == Some(asked))
        else {
            return Afterwards::Nothing;
        };
        tracing::warn!(prompt = dialog.prompt, %error, "no view for a prompt");
        Afterwards::Dismiss(Rc::clone(&dialog.dismissed))
    }

    fn index_of(&self, prompt: PromptId) -> Option<usize> {
        self.dialogs
            .iter()
            .position(|dialog| dialog.prompt == prompt)
    }
}

impl Dialog {
    /// The dialog is `metrics` now: the layer is placed at them, and the view takes them.
    fn resized(&mut self, metrics: ViewMetrics, app: &App) {
        self.metrics = Some(metrics);
        let place = CanvasPlace::filling(metrics.physical_size, metrics.device_pixel_ratio);
        place_in_context(app, &self.context, &self.layer, &place, None, || {});
        if let Some(view) = &self.view
            && let Some(embedder) = app.platform().view_embedder()
        {
            embedder.set_view_metrics(view.id(), metrics);
        }
    }

    fn set_in_sight(&mut self, in_sight: bool, app: &App) {
        self.in_sight = in_sight;
        if let Some(view) = &self.view
            && let Some(embedder) = app.platform().view_embedder()
        {
            embedder.set_view_in_sight(view.id(), in_sight);
        }
    }

    /// A pointer over the dialog, reported for the view under the number WaOS gives its device.
    fn pointer(&mut self, pointer: &PointerReport, app: &App) {
        let Some(view) = &self.view else {
            return;
        };
        let Some(device) = self.devices.number(pointer.device, &pointer.change) else {
            return;
        };
        if let Some(embedder) = app.platform().view_embedder() {
            embedder.send_pointer_reports(vec![PointerReport {
                view_id: view.id(),
                device,
                ..*pointer
            }]);
        }
    }

    fn focus(&mut self, event: ViewFocusEvent, app: &App) {
        self.focus = Some(event);
        if let Some(view) = &self.view {
            send_focus(app, view, event);
        }
    }

    /// Gives `view`, just arrived, what the dialog reported while it was asked for.
    fn give_reported(&self, view: &ViewRef, app: &App) {
        let platform = app.platform();
        let Some(embedder) = platform.view_embedder() else {
            return;
        };
        if let Some(metrics) = self.metrics
            && view.metrics() != metrics
        {
            embedder.set_view_metrics(view.id(), metrics);
        }
        if !self.in_sight {
            embedder.set_view_in_sight(view.id(), false);
        }
        if let Some(event) = self.focus {
            send_focus(app, view, event);
        }
    }
}

impl Afterwards {
    fn carry_out(self, app: &mut App) {
        match self {
            Afterwards::Nothing => {}
            Afterwards::Key(key) => {
                KeyEventManager::instance(app).handle_key_data(app, key);
            }
            Afterwards::Dismiss(dismissed) => dismissed(app),
        }
    }
}

/// What WaOS's view of a prompt asks of the system, as its dialog answers it: its cursor and its
/// focus are the dialog's; it takes no text, and has no system bars over it.
struct DialogHost {
    process: Arc<WindowProcess>,
    prompt: PromptId,
}

impl ViewHost for DialogHost {
    fn text_input(&self) -> Option<&dyn TextInputHost> {
        None
    }

    /// The dialog's, whichever device asks: its app has one cursor.
    fn activate_system_cursor(&self, _device: i64, kind: SystemMouseCursorKind) {
        self.process
            .call_prompt(self.prompt, PromptCall::SetCursor(kind));
    }

    fn request_focus_change(&self, state: ViewFocusState, direction: ViewFocusDirection) {
        self.process
            .call_prompt(self.prompt, PromptCall::ChangeFocus { state, direction });
    }

    fn system_bars(&self) -> Option<&dyn waft::ui::SystemBars> {
        None
    }
}

/// `event`, which the dialog reported of its own view, for `view`.
fn send_focus(app: &App, view: &ViewRef, event: ViewFocusEvent) {
    if let Some(embedder) = app.platform().view_embedder() {
        embedder.send_view_focus_event(ViewFocusEvent {
            view_id: view.id(),
            ..event
        });
    }
}

/// Removes `view`, one of the app's own on a prompt's layer.
fn remove_view(app: &App, view: ViewId) {
    if let Some(embedder) = app.platform().view_embedder() {
        embedder.remove_view(view);
    }
}

/// What Waft reports of the views, passed on to the dialogs.
struct DialogViews {
    shown: WeakEntity<ShownDialogs>,
}

impl WidgetsBindingObserver for DialogViews {
    fn did_add_view(&self, app: &mut App, view: &ViewRef) {
        if let Some(shown) = self.shown.upgrade() {
            shown.update(app, |shown, cx| shown.view_added(view, cx));
        }
    }

    fn did_remove_view(&self, app: &mut App, view: ViewId) {
        if let Some(shown) = self.shown.upgrade() {
            shown.update(app, |shown, cx| shown.view_removed(view, cx));
        }
    }

    fn did_fail_view_request(&self, app: &mut App, request: RequestId, error: &ViewError) {
        if let Some(shown) = self.shown.upgrade() {
            let afterwards =
                shown.update(app, |shown, _| shown.view_request_failed(request, error));
            afterwards.carry_out(app);
        }
    }
}

/// One task takes what every dialog reports, on the main thread, for as long as the dialogs are
/// there.
fn take_reports(
    app: &mut App,
    shown: &Entity<ShownDialogs>,
    mut reports: UnboundedReceiver<DialogReport>,
) {
    let shown = shown.downgrade();
    app.spawn(async move |cx| {
        while let Some(DialogReport { prompt, report }) = reports.next().await {
            let Some(shown) = shown.upgrade() else {
                break;
            };
            cx.update(|app| {
                let afterwards = shown.update(app, |shown, cx| shown.reported(prompt, report, cx));
                afterwards.carry_out(app);
            });
        }
    });
}

/// One task takes each prompt away once its answer is dropped, on the main thread, wherever that
/// happens.
fn take_away_when_dropped(
    app: &mut App,
    shown: &Entity<ShownDialogs>,
    mut taken_away: UnboundedReceiver<PromptId>,
) {
    let shown = shown.downgrade();
    app.spawn(async move |cx| {
        while let Some(prompt) = taken_away.next().await {
            let Some(shown) = shown.upgrade() else {
                break;
            };
            cx.update(|app| shown.update(app, |shown, cx| shown.hide(prompt, cx)));
        }
    });
}

#[cfg(test)]
mod tests;
