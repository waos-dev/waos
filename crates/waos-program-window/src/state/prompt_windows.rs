//! WaOS's prompts placed in WaOS's own windows: over each of the program's windows, dimming it,
//! where WaOS shows the program's windows itself, the window reserved for its first request among
//! them; and, for a program with none of them, in a window of its own, with the program's name, icon and identity, as its own windows have them, whose
//! closing dismisses the prompt, which Waft is
//! asked for, noted by what Waft answers, and which arrives as every view does, carrying
//! [`PROMPT_WINDOW`] as its initial data. Whoever builds the process's views puts the prompt in it
//! ([`PromptWindows::content_of`]). A prompt is taken away, and its window of its own closed, on
//! the main thread once its answer is dropped, wherever that happens.
//!
//! The invariant: a window of a prompt's own is open exactly while its prompt is shown, one that
//! arrives after its prompt was taken away, or that the system restored with no prompt behind it,
//! closing unseen; and the prompts notify when one comes or goes, or its window arrives, so the
//! windows and the sheets over the programs' windows rebuild then.

use std::rc::Rc;

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedSender};
use futures::channel::oneshot;
use waft::ui::{RequestId, ViewId, ViewRef};
use waft::{
    App, Context, Entity, IntoWidget, LinuxWindowConfig, WeakEntity, WidgetRef, WidgetsBinding,
    WidgetsBindingObserver, WindowConfig,
};
use waos::RunningProgram;

use crate::link::ProgramFace;
use crate::logic::window_config::windows_config_of;
use crate::prompt_placement::{PROMPT_SIZE, PromptPlacement, ShownPrompt};
use crate::state::view_windows::ViewWindows;
use crate::ui::program_windows::OverWindow;
use crate::ui::prompt_windows::PromptOver;

/// The initial data of a prompt's window of its own, which tells it from the other views.
pub const PROMPT_WINDOW: &str = "waos-prompt";

/// The placement of the prompts in WaOS's own windows.
pub struct PromptWindows {
    shown: Entity<ShownPrompts>,
    /// Where an answer dropped says which prompt to take away.
    hidden: UnboundedSender<PromptId>,
}

/// Names a prompt shown.
type PromptId = u64;

/// The prompts shown, in the order shown.
pub(crate) struct ShownPrompts {
    view_windows: Entity<ViewWindows<RunningProgram>>,
    /// What a prompt's window of its own shows of its program: its name, as its user knows it,
    /// its icon and its identity.
    face_of: Rc<dyn Fn(&RunningProgram) -> ProgramFace>,
    prompts: Vec<Shown>,
    last_id: PromptId,
}

/// A prompt shown, for its program.
pub(crate) struct Shown {
    id: PromptId,
    pub(crate) program: RunningProgram,
    pub(crate) prompt: WidgetRef,
    /// Its window of its own, where its program had no window to show it over.
    pub(crate) window: Option<OwnWindow>,
}

/// A prompt's window of its own.
pub(crate) struct OwnWindow {
    /// What Waft answered the request for the window with, which its view arrives carrying.
    asked: RequestId,
    /// The window's view, once it arrived.
    view: Option<ViewRef>,
    /// What closing the window does: dismisses the prompt.
    dismissed: Rc<dyn Fn(&mut App)>,
    /// Who waits for the window's view, until it arrives.
    arrival: Option<oneshot::Sender<ViewId>>,
}

impl PromptWindows {
    /// The placement, over the programs' windows of `view_windows`, its windows of their own showing
    /// the program as `face_of` says.
    pub fn new(
        app: &mut App,
        view_windows: Entity<ViewWindows<RunningProgram>>,
        face_of: Rc<dyn Fn(&RunningProgram) -> ProgramFace>,
    ) -> PromptWindows {
        let shown = app.new_entity(|_| ShownPrompts {
            view_windows,
            face_of,
            prompts: Vec::new(),
            last_id: 0,
        });
        let arrivals = Rc::new(WindowArrivals {
            shown: shown.downgrade(),
        });
        WidgetsBinding::instance(app).add_observer(app, arrivals);
        let (hidden, mut taken_away) = mpsc::unbounded();
        let shown_prompts = shown.downgrade();
        app.spawn(async move |cx| {
            while let Some(id) = taken_away.next().await {
                let Some(shown) = shown_prompts.upgrade() else {
                    break;
                };
                cx.update(|app| shown.update(app, |shown, cx| shown.hide(id, cx)));
            }
        });
        PromptWindows { shown, hidden }
    }

    /// Shows `prompt` for `program`, as [`PromptPlacement::show`] does, and answers it with what
    /// resolves with the Waft view of the window of its own it is shown in, once that arrives: the
    /// screen a dialog of a program with none of its own is shown over, on a system that shows a
    /// dialog only over a screen (decision 11 of
    /// `docs/proposals/2026-11-21-views-a-program-asks-for.md`). It resolves with nothing where the
    /// program has a screen after all, or Waft makes no window.
    pub fn show_on_a_screen_of_its_own(
        &self,
        app: &mut App,
        program: &RunningProgram,
        prompt: WidgetRef,
        dismissed: Rc<dyn Fn(&mut App)>,
    ) -> (ShownPrompt, oneshot::Receiver<ViewId>) {
        let (arrival, arrived) = oneshot::channel();
        let id = self.shown.update(app, |shown, cx| {
            shown.show_with(program, prompt, dismissed, Some(arrival), cx)
        });
        let hidden = self.hidden.clone();
        let shown = ShownPrompt::new(move || {
            let _ = hidden.unbounded_send(id);
        });
        (shown, arrived)
    }

    /// What `ProgramWindows` puts over each of a program's windows: its prompt shown, if any.
    pub fn over_window(&self) -> OverWindow<RunningProgram> {
        let shown = self.shown.clone();
        Rc::new(move |_app, program| {
            PromptOver {
                shown: shown.clone(),
                program: program.clone(),
            }
            .into_widget()
        })
    }

    /// What goes in the window of `view`, where it is a prompt's window of its own: the prompt;
    /// none for any other view.
    pub fn content_of(&self, app: &App, view: &ViewRef) -> Option<WidgetRef> {
        let shown = self.shown.read(app);
        let prompt = shown.shown_in(view.id())?;
        Some(prompt.prompt.clone())
    }

    /// The user asked to close the window of `view`: where it is a prompt's window of its own, the
    /// prompt is dismissed. Answers whether it was.
    pub fn close_requested(&self, app: &mut App, view: ViewId) -> bool {
        let dismissed = self
            .shown
            .read(app)
            .shown_in(view)
            .and_then(|shown| shown.window.as_ref())
            .map(|window| Rc::clone(&window.dismissed));
        if let Some(dismissed) = &dismissed {
            dismissed(app);
        }
        dismissed.is_some()
    }
}

impl PromptPlacement for PromptWindows {
    /// Over the program's windows where WaOS shows some; in a window of its own where it shows
    /// none.
    fn show(
        &self,
        app: &mut App,
        program: &RunningProgram,
        prompt: WidgetRef,
        dismissed: Rc<dyn Fn(&mut App)>,
    ) -> ShownPrompt {
        let id = self
            .shown
            .update(app, |shown, cx| shown.show(program, prompt, dismissed, cx));
        let hidden = self.hidden.clone();
        ShownPrompt::new(move || {
            let _ = hidden.unbounded_send(id);
        })
    }
}

impl ShownPrompts {
    /// Each prompt shown, in the order shown.
    pub(crate) fn prompts(&self) -> &[Shown] {
        &self.prompts
    }

    /// The prompt shown in the window of `view`, its own.
    fn shown_in(&self, view: ViewId) -> Option<&Shown> {
        self.prompts.iter().find(|shown| {
            let own = shown
                .window
                .as_ref()
                .and_then(|window| window.view.as_ref());
            own.is_some_and(|own| own.id() == view)
        })
    }

    fn show(
        &mut self,
        program: &RunningProgram,
        prompt: WidgetRef,
        dismissed: Rc<dyn Fn(&mut App)>,
        cx: &mut Context<ShownPrompts>,
    ) -> PromptId {
        self.show_with(program, prompt, dismissed, None, cx)
    }

    /// Shows `prompt` as `show` does, `arrival` hearing the view of its window of its own once it
    /// arrives, where it has one.
    fn show_with(
        &mut self,
        program: &RunningProgram,
        prompt: WidgetRef,
        dismissed: Rc<dyn Fn(&mut App)>,
        arrival: Option<oneshot::Sender<ViewId>>,
        cx: &mut Context<ShownPrompts>,
    ) -> PromptId {
        self.last_id += 1;
        let window = if self.view_windows.read(cx).has_a_screen(program) {
            None
        } else {
            self.open_window(program, dismissed, arrival, cx)
        };
        self.prompts.push(Shown {
            id: self.last_id,
            program: program.clone(),
            prompt,
            window,
        });
        cx.notify();
        self.last_id
    }

    /// Asks for a window of a prompt's own, for `program`; closing it is `dismissed`. None where
    /// Waft makes no window, the prompt then shown nowhere until it is answered elsewhere.
    fn open_window(
        &self,
        program: &RunningProgram,
        dismissed: Rc<dyn Fn(&mut App)>,
        arrival: Option<oneshot::Sender<ViewId>>,
        cx: &mut Context<ShownPrompts>,
    ) -> Option<OwnWindow> {
        let face = (self.face_of)(program);
        let identity = face.identity.as_ref();
        let config = WindowConfig {
            title: face.name.clone(),
            size: PROMPT_SIZE,
            resizable: false,
            icon: face.icon.clone(),
            windows: windows_config_of(identity),
            linux: LinuxWindowConfig {
                app_id: identity.map(|identity| identity.app_id.clone()),
                activation_token: None,
            },
            ..WindowConfig::default()
        };
        match cx
            .platform()
            .request_view(config, Some(PROMPT_WINDOW.to_owned()))
        {
            Ok(asked) => Some(OwnWindow {
                asked,
                view: None,
                dismissed,
                arrival,
            }),
            Err(error) => {
                tracing::warn!(%error, "a prompt's window of its own was refused");
                None
            }
        }
    }

    /// Waft added `view`: a prompt's window of its own shows its prompt from now on; one whose
    /// prompt was taken away, or that answers no request of this process, closes unseen.
    fn view_added(&mut self, view: &ViewRef, cx: &mut Context<ShownPrompts>) {
        if view.initial_data().as_deref() != Some(PROMPT_WINDOW) {
            return;
        }
        let asked = view.request_id();
        let own = self
            .prompts
            .iter_mut()
            .filter_map(|shown| shown.window.as_mut())
            .find(|window| Some(window.asked) == asked);
        match own {
            Some(own) => {
                own.view = Some(Rc::clone(view));
                if let Some(arrival) = own.arrival.take() {
                    // Whoever waited may have stopped waiting.
                    let _ = arrival.send(view.id());
                }
                cx.notify();
            }
            None => close_window_of(view),
        }
    }

    /// Waft removed `view`: where it was a prompt's window of its own, the prompt stays shown with
    /// no window until it is answered elsewhere.
    fn view_removed(&mut self, view: ViewId, cx: &mut Context<ShownPrompts>) {
        let own = self
            .prompts
            .iter_mut()
            .filter_map(|shown| shown.window.as_mut())
            .find(|window| window.view.as_ref().is_some_and(|own| own.id() == view));
        if let Some(own) = own {
            own.view = None;
            cx.notify();
        }
    }

    /// Takes the prompt `id` away, closing its window of its own, or leaving one still coming to
    /// close as it arrives.
    fn hide(&mut self, id: PromptId, cx: &mut Context<ShownPrompts>) {
        let Some(index) = self.prompts.iter().position(|shown| shown.id == id) else {
            return;
        };
        let hidden = self.prompts.remove(index);
        if let Some(view) = hidden.window.and_then(|window| window.view) {
            close_window_of(&view);
        }
        cx.notify();
    }
}

/// Closes the window `view` is in, where it is in one.
fn close_window_of(view: &ViewRef) {
    if let Some(window) = view.window() {
        window.close();
    }
}

/// What Waft reports of the views added and removed, passed on to the prompts.
struct WindowArrivals {
    shown: WeakEntity<ShownPrompts>,
}

impl WidgetsBindingObserver for WindowArrivals {
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
}
