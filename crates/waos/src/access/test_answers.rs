//! Grants for the tests of what is asked: the answers to the questions and to link schemes, as a
//! test sets them, and nothing else granted.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use waos_capabilities::{
    Answer, ChangedFuture, FileSystemGrant, NetworkGrant, ProgramGrants, Question,
};

use super::ProgramAccess;
use crate::sync::locked;

/// The grants a test answers.
#[derive(Default)]
pub(crate) struct Answers {
    questions: Mutex<HashMap<Question, Answer>>,
    link_schemes: Mutex<HashMap<String, Answer>>,
}

impl Answers {
    /// The access of a program with these answers.
    pub(crate) fn access(self: &Arc<Self>) -> ProgramAccess {
        ProgramAccess::new(Arc::clone(self) as Arc<dyn ProgramGrants>)
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
    }

    fn link_scheme_answer(&self, scheme: &str) -> Option<Answer> {
        locked(&self.link_schemes).get(scheme).copied()
    }

    fn set_link_scheme_answer(&self, scheme: &str, answer: Answer) {
        locked(&self.link_schemes).insert(scheme.to_owned(), answer);
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(std::future::pending())
    }
}
