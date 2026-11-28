//! The questions apps put to their user, until each is answered: whether an app may do something,
//! or which output its sound plays on, picked from the outputs the engine lists. One at a time for
//! each app, by its id, whether or not an instance of it runs, in one prompt, the question sheet,
//! which a `PromptPlacement` shows for the app wherever the system's shell puts what the app
//! shows (in WaOS's own windows, over the app's windows, or in a window of the prompt's own for an
//! app with none; on macOS, in a dialog of the app's window process). The reply goes back to
//! whoever asked: a question's to the app's permissions, which keep its answer in its settings, an
//! output's to the engine, which lets the app play on the output picked; a question whose asker
//! stopped waiting for it, as when the instance that asked ended or a setting answered it, or whose
//! prompt the user dismissed, goes unanswered. A prompt may ask nothing and only tell the user
//! something, read once they reply: that WaOS still may not reach the phone's files
//! (`state::phone_files`).
//!
//! The invariant: every question taken is answered, or dropped unanswered, exactly once, and an
//! app's prompt is shown exactly while it has a question waiting.

#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use waft::{App, Context, Entity, IntoWidget, WeakEntity};
use waos_package::ProgramId;
use waos_program_window::{PromptPlacement, PromptWindows, ShownPrompt};
use waos_services::packages::PackageManager;

use crate::logic::program_faces::ProgramFaces;
use crate::logic::program_name::name_of;
use crate::logic::prompts::{Asked, Asking, Reply};
use crate::state::ViewWindows;
use crate::ui::question_sheet::QuestionSheet;

pub(crate) struct Questions {
    /// The questions waiting for an answer, in the order asked.
    asked: Vec<Pending>,
    next_id: u64,
    /// Where each app's prompt is shown.
    placement: Rc<dyn PromptPlacement>,
    /// The prompt of each app with a question waiting.
    shown: Vec<(ProgramId, ShownPrompt)>,
    /// Where an installed app's name is read.
    packages: Arc<PackageManager>,
}

/// A question waiting for its reply.
pub(crate) struct Pending {
    id: u64,
    /// The app it is about.
    pub(crate) app: ProgramId,
    /// The app's name, as its user knows it.
    pub(crate) name: String,
    /// What it asks, with where the reply goes.
    pub(crate) asking: Asking,
}

impl Questions {
    /// The questions, which take each arriving on `asked` for as long as questions can
    /// arrive, each app's prompt shown by `placement`; an installed app's name is read from
    /// `packages`.
    pub(crate) fn listen(
        app: &mut App,
        asked: UnboundedReceiver<Asked>,
        placement: Rc<dyn PromptPlacement>,
        packages: Arc<PackageManager>,
    ) -> Entity<Questions> {
        let questions = app.new_entity(|_cx| Questions {
            asked: Vec::new(),
            next_id: 1,
            placement,
            shown: Vec::new(),
            packages,
        });
        take_questions(app, questions.clone(), asked);
        questions
    }

    /// The question `app`'s user is asked now: its first one waiting.
    pub(crate) fn asked_of(&self, app: &ProgramId) -> Option<&Pending> {
        self.asked.iter().find(|pending| &pending.app == app)
    }

    /// `app`'s user replied to its question asked now, or dismissed it with no reply.
    pub(crate) fn reply(
        &mut self,
        app: &ProgramId,
        reply: Option<Reply>,
        cx: &mut Context<Questions>,
    ) {
        self.take_first_of(app, reply, cx);
    }

    /// A question arrived: it waits its turn in its app's prompt, shown now if the app had no
    /// question waiting, and its id is answered so the caller can close it when cancelled.
    fn arrived(&mut self, app: ProgramId, asking: Asking, cx: &mut Context<Questions>) -> u64 {
        tracing::info!(%app, ?asking, "an app asks its user");
        if self.asked_of(&app).is_none() {
            self.show_prompt(&app, cx);
        }
        let id = self.next_id;
        self.next_id += 1;
        let name = name_of(&self.packages, app.as_str());
        self.asked.push(Pending {
            id,
            app,
            name,
            asking,
        });
        cx.notify();
        id
    }

    /// Whoever asked question `id` stopped waiting for it (for instance, because a setting answered
    /// it, or the instance that asked ended): remove it and hide the app's prompt if no question
    /// remains for it.
    fn cancelled(&mut self, id: u64, cx: &mut Context<Questions>) {
        let Some(index) = self.asked.iter().position(|pending| pending.id == id) else {
            return;
        };
        let pending = self.asked.remove(index);
        tracing::info!(app = %pending.app, asking = ?pending.asking, "cancelled");
        if self.asked_of(&pending.app).is_none() {
            self.shown
                .retain(|(shown_for, _)| shown_for != &pending.app);
        }
        cx.notify();
    }

    /// `app`'s questions go unanswered: its user dismissed its prompt.
    fn drop_all_of(&mut self, app: &ProgramId, cx: &mut Context<Questions>) {
        while self.asked_of(app).is_some() {
            self.take_first_of(app, None, cx);
        }
    }

    /// Replies `reply` to `app`'s first question, and shows the next, or takes the app's prompt
    /// away when none is left.
    fn take_first_of(
        &mut self,
        app: &ProgramId,
        reply: Option<Reply>,
        cx: &mut Context<Questions>,
    ) {
        let Some(index) = self.asked.iter().position(|pending| &pending.app == app) else {
            return;
        };
        let pending = self.asked.remove(index);
        tracing::info!(%app, asking = ?pending.asking, ?reply, "answered");
        pending.asking.reply(reply);
        if self.asked_of(app).is_none() {
            self.shown.retain(|(shown_for, _)| shown_for != app);
        }
        cx.notify();
    }

    /// Shows `app`'s prompt, the question sheet, through the placement; the user dismissing it
    /// leaves the app's questions unanswered.
    fn show_prompt(&mut self, app: &ProgramId, cx: &mut Context<Questions>) {
        let sheet = QuestionSheet {
            questions: cx.entity(),
            app: app.clone(),
        }
        .into_widget();
        let (questions, unanswered) = (cx.weak_entity(), app.clone());
        let dismissed = Rc::new(move |app: &mut App| {
            if let Some(questions) = questions.upgrade() {
                questions.update(app, |questions, cx| questions.drop_all_of(&unanswered, cx));
            }
        });
        let placement = Rc::clone(&self.placement);
        let shown = placement.show(cx, app, sheet, dismissed);
        self.shown.push((app.clone(), shown));
    }
}

/// One task takes every question, on the main thread, and keeps the questions for as long as one
/// can arrive; its asker dropping its wait drops what it asks.
fn take_questions(
    app: &mut App,
    questions: Entity<Questions>,
    mut asked: UnboundedReceiver<Asked>,
) {
    app.spawn(async move |cx| {
        while let Some(question) = asked.next().await {
            let Asked {
                app: asking_app,
                asking,
                cancelled,
            } = question;
            cx.update(|app| {
                let id = questions.update(app, |questions, cx| {
                    questions.arrived(asking_app, asking, cx)
                });
                close_when_cancelled(app, id, cancelled, questions.downgrade());
            });
        }
    });
}

/// Removes `id` once the engine drops its wait on it.
fn close_when_cancelled(
    app: &mut App,
    id: u64,
    cancelled: oneshot::Receiver<()>,
    questions: WeakEntity<Questions>,
) {
    app.spawn(async move |cx| {
        let _ = cancelled.await;
        cx.update(|app| {
            if let Some(questions) = questions.upgrade() {
                questions.update(app, |questions, cx| questions.cancelled(id, cx));
            }
        });
    });
}

/// WaOS's own windows as the placement of the apps' prompts: over the apps' windows of
/// `view_windows`, or in a window of a prompt's own, showing its app as `faces` does.
pub(crate) fn prompt_windows(
    app: &mut App,
    view_windows: Entity<ViewWindows>,
    faces: ProgramFaces,
) -> Rc<PromptWindows> {
    let face_of = Rc::new(move |app: &ProgramId| faces.face(app.as_str()));
    Rc::new(PromptWindows::new(app, view_windows, face_of))
}
