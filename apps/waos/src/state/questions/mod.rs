//! The questions programs put to their user, until each is answered: whether a program may do
//! something, or which output its sound plays on, picked from the outputs the engine lists. One at
//! a time for each program, in one prompt, the question sheet, which a `PromptPlacement` shows for
//! the program (in WaOS's own windows, over the program's windows, or in a window of the prompt's
//! own for a program with none; on macOS, in a dialog of the program's app). The reply goes back to
//! the engine, which keeps a question's answer in the program's settings and lets the program play
//! on the output picked; a question whose program ended, or whose prompt the user dismissed, goes
//! unanswered.
//!
//! The invariant: every question taken is answered, or dropped unanswered, exactly once, and a
//! program's prompt is shown exactly while it has a question waiting.

#[cfg(test)]
mod tests;

use std::rc::Rc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waft::{App, Context, Entity, IntoWidget, WeakEntity};
use waos::RunningProgram;
use waos_program_window::{PromptPlacement, PromptWindows, ShownPrompt};
use waos_store::Store;

use crate::logic::program_faces::ProgramFaces;
use crate::logic::program_name::program_name;
use crate::logic::prompts::{Asked, Asking, Reply};
use crate::state::ViewWindows;
use crate::ui::question_sheet::QuestionSheet;

pub(crate) struct Questions {
    /// The questions waiting for an answer, in the order asked.
    asked: Vec<Pending>,
    /// Where each program's prompt is shown.
    placement: Rc<dyn PromptPlacement>,
    /// The prompt of each program with a question waiting.
    shown: Vec<(RunningProgram, ShownPrompt)>,
    /// Where an installed program's name is read.
    store: Store,
}

/// A question waiting for its reply.
pub(crate) struct Pending {
    pub(crate) program: RunningProgram,
    /// The program's name, as its user knows it.
    pub(crate) name: String,
    /// What it asks, with where the reply goes.
    pub(crate) asking: Asking,
}

impl Questions {
    /// The questions, which take each one arriving on `asked` for as long as questions can
    /// arrive, each program's prompt shown by `placement`; an installed program's name is read
    /// from `store`.
    pub(crate) fn listen(
        app: &mut App,
        asked: UnboundedReceiver<Asked>,
        placement: Rc<dyn PromptPlacement>,
        store: Store,
    ) {
        let questions = app.new_entity(|_| Questions {
            asked: Vec::new(),
            placement,
            shown: Vec::new(),
            store,
        });
        take_questions(app, questions, asked);
    }

    /// The question `program`'s user is asked now: its first one waiting.
    pub(crate) fn asked_of(&self, program: &RunningProgram) -> Option<&Pending> {
        self.asked
            .iter()
            .find(|pending| &pending.program == program)
    }

    /// `program`'s user replied to its question asked now, or dismissed it with no reply.
    pub(crate) fn reply(
        &mut self,
        program: &RunningProgram,
        reply: Option<Reply>,
        cx: &mut Context<Questions>,
    ) {
        self.take_first_of(program, reply, cx);
    }

    /// A question arrived: it waits its turn in its program's prompt, shown now if the program
    /// had no question waiting.
    fn arrived(&mut self, asked: Asked, cx: &mut Context<Questions>) {
        let Asked { program, asking } = asked;
        tracing::info!(program = program.name(), ?asking, "a program asks its user");
        if self.asked_of(&program).is_none() {
            self.show_prompt(&program, cx);
        }
        let name = program_name(&self.store, &program);
        self.asked.push(Pending {
            program,
            name,
            asking,
        });
        cx.notify();
    }

    /// `program`'s questions go unanswered: it ended, or its user dismissed its prompt.
    fn drop_all_of(&mut self, program: &RunningProgram, cx: &mut Context<Questions>) {
        while self.asked_of(program).is_some() {
            self.take_first_of(program, None, cx);
        }
    }

    /// Replies `reply` to `program`'s first question, and shows the next, or takes the program's
    /// prompt away when none is left.
    fn take_first_of(
        &mut self,
        program: &RunningProgram,
        reply: Option<Reply>,
        cx: &mut Context<Questions>,
    ) {
        let Some(index) = self
            .asked
            .iter()
            .position(|pending| &pending.program == program)
        else {
            return;
        };
        let pending = self.asked.remove(index);
        tracing::info!(program = program.name(), asking = ?pending.asking, ?reply, "answered");
        pending.asking.reply(reply);
        if self.asked_of(program).is_none() {
            self.shown.retain(|(shown_for, _)| shown_for != program);
        }
        cx.notify();
    }

    /// Shows `program`'s prompt, the question sheet, through the placement; the user dismissing
    /// it leaves the program's questions unanswered.
    fn show_prompt(&mut self, program: &RunningProgram, cx: &mut Context<Questions>) {
        let sheet = QuestionSheet {
            questions: cx.entity(),
            program: program.clone(),
        }
        .into_widget();
        let (questions, unanswered) = (cx.weak_entity(), program.clone());
        let dismissed = Rc::new(move |app: &mut App| {
            if let Some(questions) = questions.upgrade() {
                questions.update(app, |questions, cx| questions.drop_all_of(&unanswered, cx));
            }
        });
        let placement = Rc::clone(&self.placement);
        let shown = placement.show(cx, program, sheet, dismissed);
        self.shown.push((program.clone(), shown));
    }
}

/// One task takes every question the programs ask, on the main thread, and keeps the questions
/// for as long as one can arrive; each question's program ending drops what it still asks.
fn take_questions(
    app: &mut App,
    questions: Entity<Questions>,
    mut asked: UnboundedReceiver<Asked>,
) {
    app.spawn(async move |cx| {
        while let Some(question) = asked.next().await {
            cx.update(|app| {
                let program = question.program.clone();
                questions.update(app, |questions, cx| questions.arrived(question, cx));
                close_when_ended(app, &program, questions.downgrade());
            });
        }
    });
}

/// Drops `program`'s questions once it has ended.
fn close_when_ended(app: &mut App, program: &RunningProgram, questions: WeakEntity<Questions>) {
    let (ended, program) = (program.ended(), program.clone());
    app.spawn(async move |cx| {
        ended.await;
        cx.update(|app| {
            if let Some(questions) = questions.upgrade() {
                questions.update(app, |questions, cx| questions.drop_all_of(&program, cx));
            }
        });
    });
}

/// WaOS's own windows as the placement of the programs' prompts: over the programs' windows of
/// `view_windows`, or in a window of a prompt's own, showing its program as `faces` does.
pub(crate) fn prompt_windows(
    app: &mut App,
    view_windows: Entity<ViewWindows>,
    faces: ProgramFaces,
) -> Rc<PromptWindows> {
    let face_of = Rc::new(move |program: &RunningProgram| faces.face(program.name()));
    Rc::new(PromptWindows::new(app, view_windows, face_of))
}
