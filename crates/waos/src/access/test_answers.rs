//! Grants for the tests of what is asked: the answers to the questions, to link schemes and to
//! roles, and whether a record is kept, as a test sets them, each set heard as a change, and
//! nothing else granted.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use waos_capabilities::{
    Answer, ChangedFuture, FileSystemGrant, NetworkGrant, ProgramGrants, Question, Role,
};

use super::ProgramAccess;
use crate::sync::locked;

/// The grants a test answers.
#[derive(Default)]
pub(crate) struct Answers {
    questions: Mutex<HashMap<Question, Answer>>,
    link_schemes: Mutex<HashMap<String, Answer>>,
    roles: Mutex<HashMap<Role, Answer>>,
    /// Whether the record of what the program does is off.
    record_off: Mutex<bool>,
    /// Who waits for the next change.
    waiting: Mutex<Vec<oneshot::Sender<()>>>,
}

impl Answers {
    /// The access of a program with these answers.
    pub(crate) fn access(self: &Arc<Self>) -> ProgramAccess {
        ProgramAccess::new(Arc::clone(self) as Arc<dyn ProgramGrants>)
    }

    /// The user turns the record of what the program does on or off.
    pub(crate) fn keep_record(&self, on: bool) {
        *locked(&self.record_off) = !on;
        self.changed_now();
    }

    fn changed_now(&self) {
        for waiting in locked(&self.waiting).drain(..) {
            let _ = waiting.send(());
        }
    }
}

impl ProgramGrants for Answers {
    fn network(&self) -> NetworkGrant {
        crate::permissions::NothingGranted.network()
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant::default()
    }

    fn answer(&self, question: Question) -> Option<Answer> {
        locked(&self.questions).get(&question).copied()
    }

    fn set_answer(&self, question: Question, answer: Answer) {
        locked(&self.questions).insert(question, answer);
        self.changed_now();
    }

    fn link_scheme_answer(&self, scheme: &str) -> Option<Answer> {
        locked(&self.link_schemes).get(scheme).copied()
    }

    fn set_link_scheme_answer(&self, scheme: &str, answer: Answer) {
        locked(&self.link_schemes).insert(scheme.to_owned(), answer);
        self.changed_now();
    }

    fn role(&self, role: Role) -> Option<Answer> {
        locked(&self.roles).get(&role).copied()
    }

    fn set_role(&self, role: Role, answer: Answer) {
        locked(&self.roles).insert(role, answer);
        self.changed_now();
    }

    fn keeps_record(&self) -> bool {
        !*locked(&self.record_off)
    }

    fn changed(&self) -> ChangedFuture {
        let (changed, heard) = oneshot::channel();
        locked(&self.waiting).push(changed);
        Box::pin(async move {
            let _ = heard.await;
        })
    }
}
