//! The user, as a test plays them, answering what the engine asks: each question, each link
//! scheme a program asks to open, each role it asks for, kept until the test answers it; and each
//! settings page a program opens, until the test leaves it.

use std::sync::{Mutex, MutexGuard, PoisonError};

use futures::FutureExt;
use futures::channel::oneshot;
use waos::{
    Answer, AnswerFuture, Prompts, Question, Role, RunningProgram, SettingsClosedFuture, Switch,
};

/// What the engine asked the user, and what is still waiting for an answer.
#[derive(Default)]
pub struct TestPrompts {
    questions: Mutex<Vec<Put<Question>>>,
    link_schemes: Mutex<Vec<Put<String>>>,
    roles: Mutex<Vec<Put<Role>>>,
    settings: Mutex<Vec<OpenedSettings>>,
}

/// One thing put to the user by a program, and its answer while it waits.
struct Put<T> {
    program: String,
    asked: T,
    answer: Option<oneshot::Sender<Option<Answer>>>,
}

/// One settings page a program opened, and its leaving while it is open.
struct OpenedSettings {
    program: String,
    switch: Switch,
    leave: Option<oneshot::Sender<()>>,
}

impl TestPrompts {
    /// Each question put to the user so far, with the program that put it, in order.
    pub fn questions(&self) -> Vec<(String, Question)> {
        asked(&self.questions, |question| *question)
    }

    /// The user answers every question waiting with `answer`.
    pub fn answer_questions(&self, answer: Option<Answer>) {
        answer_all(&self.questions, answer);
    }

    /// Whether the `index`th question's wait was dropped without its answer.
    pub fn question_cancelled(&self, index: usize) -> bool {
        cancelled(&self.questions, index)
    }

    /// Each link scheme a program asked its user to open so far, with the program, in order.
    pub fn link_schemes_asked(&self) -> Vec<(String, String)> {
        asked(&self.link_schemes, String::clone)
    }

    /// The user answers every link scheme waiting with `answer`.
    pub fn answer_link_schemes(&self, answer: Option<Answer>) {
        answer_all(&self.link_schemes, answer);
    }

    /// Whether the `index`th link scheme's wait was dropped without its answer.
    pub fn link_scheme_cancelled(&self, index: usize) -> bool {
        cancelled(&self.link_schemes, index)
    }

    /// Each role a program asked its user for so far, with the program, in order.
    pub fn roles_asked(&self) -> Vec<(String, Role)> {
        asked(&self.roles, |role| *role)
    }

    /// The user answers every role waiting with `answer`.
    pub fn answer_roles(&self, answer: Option<Answer>) {
        answer_all(&self.roles, answer);
    }

    /// Whether the `index`th role's wait was dropped without its answer.
    pub fn role_cancelled(&self, index: usize) -> bool {
        cancelled(&self.roles, index)
    }

    /// Each settings page opened so far, with the program that opened it, in order.
    pub fn settings_opened(&self) -> Vec<(String, Switch)> {
        locked(&self.settings)
            .iter()
            .map(|opened| (opened.program.clone(), opened.switch))
            .collect()
    }

    /// The user leaves every settings page open.
    pub fn leave_settings(&self) {
        for opened in locked(&self.settings).iter_mut() {
            if let Some(leave) = opened.leave.take() {
                let _ = leave.send(());
            }
        }
    }
}

impl Prompts for TestPrompts {
    fn ask(&self, program: &RunningProgram, question: Question) -> AnswerFuture {
        put(&self.questions, program, question)
    }

    fn ask_to_open_links(&self, program: &RunningProgram, scheme: &str) -> AnswerFuture {
        put(&self.link_schemes, program, scheme.to_owned())
    }

    fn ask_for_role(&self, program: &RunningProgram, role: Role) -> AnswerFuture {
        put(&self.roles, program, role)
    }

    fn open_settings(&self, program: &RunningProgram, switch: Switch) -> SettingsClosedFuture {
        let (leave, left) = oneshot::channel();
        locked(&self.settings).push(OpenedSettings {
            program: program.name().to_owned(),
            switch,
            leave: Some(leave),
        });
        Box::pin(left.map(|_left| ()))
    }
}

/// Puts `asked` to the user for `program`, among `all`: answered once the test answers it, with
/// none if the test drops it.
fn put<T>(all: &Mutex<Vec<Put<T>>>, program: &RunningProgram, asked: T) -> AnswerFuture {
    let (answer, answered) = oneshot::channel();
    locked(all).push(Put {
        program: program.name().to_owned(),
        asked,
        answer: Some(answer),
    });
    Box::pin(answered.map(|answer| answer.unwrap_or(None)))
}

fn asked<T, U>(all: &Mutex<Vec<Put<T>>>, value: impl Fn(&T) -> U) -> Vec<(String, U)> {
    locked(all)
        .iter()
        .map(|put| (put.program.clone(), value(&put.asked)))
        .collect()
}

fn answer_all<T>(all: &Mutex<Vec<Put<T>>>, answer: Option<Answer>) {
    for put in locked(all).iter_mut() {
        if let Some(waiting) = put.answer.take() {
            let _ = waiting.send(answer);
        }
    }
}

fn cancelled<T>(all: &Mutex<Vec<Put<T>>>, index: usize) -> bool {
    locked(all)
        .get(index)
        .and_then(|put| put.answer.as_ref())
        .is_some_and(oneshot::Sender::is_canceled)
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
