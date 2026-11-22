//! The questions programs put to their user, until each is answered: whether a program may do
//! something, or which output its sound plays on, picked from the outputs the engine lists. One at
//! a time for each program, in one prompt, the question sheet, which a `PromptPlacement` shows for
//! the program (in WaOS's own windows, over the program's windows, or in a window of the prompt's
//! own for a program with none; on macOS, in a dialog of the program's app). The reply goes back to
//! the engine, which keeps a question's answer in the program's settings and lets the program play
//! on the output picked; a question whose program ended, or whose prompt the user dismissed, goes
//! unanswered. Where WaOS reaches the phone's files only as the user allows, a pick of This computer
//! while they are out of reach first offers the system's page for them, and is answered once the
//! user goes on without it, or comes back to WaOS from the page (`logic::phone_files`).
//!
//! The invariant: every question taken is answered, or dropped unanswered, exactly once, and a
//! program's prompt is shown exactly while it has a question waiting.

#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use waft::{App, AppLifecycleListener, Context, Entity, IntoWidget, WeakEntity};
use waos::RunningProgram;
use waos_program_window::{PromptPlacement, PromptWindows, ShownPrompt};
use waos_store::Store;

use crate::logic::phone_files::{PhoneFiles, asks_for_phone_files};
use crate::logic::program_faces::ProgramFaces;
use crate::logic::program_name::program_name;
use crate::logic::prompts::{Asked, Asking, Reply};
use crate::state::ViewWindows;
use crate::ui::question_sheet::QuestionSheet;

pub(crate) struct Questions {
    /// The questions waiting for an answer, in the order asked.
    asked: Vec<Pending>,
    next_id: u64,
    /// Where each program's prompt is shown.
    placement: Rc<dyn PromptPlacement>,
    /// The prompt of each program with a question waiting.
    shown: Vec<(RunningProgram, ShownPrompt)>,
    /// Where an installed program's name is read.
    store: Store,
    /// Whether WaOS may reach the phone's files, where it reaches them only as the user allows.
    phone_files: Option<Arc<dyn PhoneFiles>>,
}

/// A question waiting for its reply.
pub(crate) struct Pending {
    id: u64,
    pub(crate) program: RunningProgram,
    /// The program's name, as its user knows it.
    pub(crate) name: String,
    /// What it asks, with where the reply goes.
    pub(crate) asking: Asking,
    /// Where its pick of This computer stands, while the system's page for the phone's files is
    /// offered first.
    pub(crate) phone_files: Option<PhoneFilesStep>,
}

/// Where a pick of This computer stands while the system's page for the phone's files is offered
/// before it is answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhoneFilesStep {
    /// The sheet offers the page, or going on without it.
    Offered,
    /// The page is open: the pick is answered once WaOS is back in front.
    PageOpen,
}

impl Questions {
    /// The questions, which take each arriving on `asked` for as long as questions can
    /// arrive, each program's prompt shown by `placement`; an installed program's name is read
    /// from `store`, and whether WaOS may reach the phone's files from `phone_files`, where it
    /// reaches them only as the user allows.
    pub(crate) fn listen(
        app: &mut App,
        asked: UnboundedReceiver<Asked>,
        placement: Rc<dyn PromptPlacement>,
        store: Store,
        phone_files: Option<Arc<dyn PhoneFiles>>,
    ) {
        let questions = app.new_entity(|_| Questions {
            asked: Vec::new(),
            next_id: 1,
            placement,
            shown: Vec::new(),
            store,
            phone_files,
        });
        answer_picks_back_in_front(app, questions.downgrade());
        take_questions(app, questions, asked);
    }

    /// The question `program`'s user is asked now: its first one waiting.
    pub(crate) fn asked_of(&self, program: &RunningProgram) -> Option<&Pending> {
        self.asked
            .iter()
            .find(|pending| &pending.program == program)
    }

    /// `program`'s user replied to its question asked now, or dismissed it with no reply. A pick
    /// of This computer while the phone's files are out of reach waits while the system's page
    /// for them is offered.
    pub(crate) fn reply(
        &mut self,
        program: &RunningProgram,
        reply: Option<Reply>,
        cx: &mut Context<Questions>,
    ) {
        let offered = self
            .phone_files
            .as_ref()
            .is_some_and(|phone_files| asks_for_phone_files(reply.as_ref(), phone_files.as_ref()));
        if offered && self.set_phone_files_step(program, PhoneFilesStep::Offered) {
            cx.notify();
            return;
        }
        self.take_first_of(program, reply, cx);
    }

    /// `program`'s user goes on with This computer without the phone's files.
    pub(crate) fn go_on_without_phone_files(
        &mut self,
        program: &RunningProgram,
        cx: &mut Context<Questions>,
    ) {
        self.take_first_of(program, Some(this_computer()), cx);
    }

    /// `program`'s user opens the system's page for the phone's files: its pick of This computer
    /// is answered once WaOS is back in front, or at once if the page does not open.
    pub(crate) fn open_phone_files_page(
        &mut self,
        program: &RunningProgram,
        cx: &mut Context<Questions>,
    ) {
        let Some(phone_files) = &self.phone_files else {
            return;
        };
        match phone_files.open_switch() {
            Ok(()) => {
                if self.set_phone_files_step(program, PhoneFilesStep::PageOpen) {
                    cx.notify();
                }
            }
            Err(why) => {
                tracing::warn!(
                    program = program.name(),
                    why,
                    "the phone's files' page did not open"
                );
                self.go_on_without_phone_files(program, cx);
            }
        }
    }

    /// WaOS is back in front: each pick of This computer waiting on the system's page for the
    /// phone's files is answered, whatever the user chose there.
    fn back_in_front(&mut self, cx: &mut Context<Questions>) {
        let waiting: Vec<RunningProgram> = self
            .asked
            .iter()
            .filter(|pending| pending.phone_files == Some(PhoneFilesStep::PageOpen))
            .map(|pending| pending.program.clone())
            .collect();
        for program in waiting {
            self.go_on_without_phone_files(&program, cx);
        }
    }

    /// `program`'s question asked now stands at `step`; whether it has one.
    fn set_phone_files_step(&mut self, program: &RunningProgram, step: PhoneFilesStep) -> bool {
        let asked_now = self
            .asked
            .iter_mut()
            .find(|pending| &pending.program == program);
        let Some(pending) = asked_now else {
            return false;
        };
        pending.phone_files = Some(step);
        true
    }

    /// A question arrived: it waits its turn in its program's prompt, shown now if the program
    /// had no question waiting, and its id is answered so the caller can close it when cancelled.
    fn arrived(
        &mut self,
        program: RunningProgram,
        asking: Asking,
        cx: &mut Context<Questions>,
    ) -> u64 {
        tracing::info!(program = program.name(), ?asking, "a program asks its user");
        if self.asked_of(&program).is_none() {
            self.show_prompt(&program, cx);
        }
        let id = self.next_id;
        self.next_id += 1;
        let name = program_name(&self.store, &program);
        self.asked.push(Pending {
            id,
            program,
            name,
            asking,
            phone_files: None,
        });
        cx.notify();
        id
    }

    /// The engine dropped its wait on question `id` (for instance, because a setting answered it):
    /// remove it and hide the program's prompt if no question remains for it.
    fn cancelled(&mut self, id: u64, cx: &mut Context<Questions>) {
        let Some(index) = self.asked.iter().position(|pending| pending.id == id) else {
            return;
        };
        let pending = self.asked.remove(index);
        tracing::info!(
            program = pending.program.name(),
            asking = ?pending.asking,
            "cancelled"
        );
        if self.asked_of(&pending.program).is_none() {
            self.shown
                .retain(|(shown_for, _)| shown_for != &pending.program);
        }
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

/// One task takes every question, on the main thread, and keeps the questions for as long as one
/// can arrive; each question's program ending, or its caller dropping its wait, drops what it
/// asks.
fn take_questions(
    app: &mut App,
    questions: Entity<Questions>,
    mut asked: UnboundedReceiver<Asked>,
) {
    app.spawn(async move |cx| {
        while let Some(question) = asked.next().await {
            let Asked {
                program,
                asking,
                cancelled,
            } = question;
            cx.update(|app| {
                let id = questions.update(app, |questions, cx| {
                    questions.arrived(program.clone(), asking, cx)
                });
                close_when_cancelled(app, id, cancelled, questions.downgrade());
                close_when_ended(app, &program, questions.downgrade());
            });
        }
    });
}

/// This computer, as a picker of machines is answered.
fn this_computer() -> Reply {
    Reply::Machine(waos_local::THIS_COMPUTER.to_owned())
}

/// From now on, each time WaOS comes back to the front, `questions` answers the picks waiting on
/// the system's page for the phone's files, which the user leaves by coming back.
fn answer_picks_back_in_front(app: &mut App, questions: WeakEntity<Questions>) {
    let back_in_front = move |app: &mut App| {
        if let Some(questions) = questions.upgrade() {
            questions.update(app, |questions, cx| questions.back_in_front(cx));
        }
    };
    AppLifecycleListener::new(app).on_resume(app, Rc::new(back_in_front));
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
