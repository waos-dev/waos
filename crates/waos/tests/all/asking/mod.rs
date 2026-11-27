//! Grants for a program whose user is asked at the first use: nothing switched on, the answers to
//! the questions put kept as the store keeps them, none answered yet, every link scheme refused,
//! and the picker of outputs asked unless a test set its answer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use waos::{Answer, ProgramGrants, Question};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant};

/// Grants that switch nothing on and keep the answers to the questions put, none answered yet.
#[derive(Default)]
pub struct QuestionsAsked {
    answers: Mutex<HashMap<Question, Answer>>,
    /// The output, by its id, set as the picker of outputs' answer.
    audio_output: Mutex<Option<String>>,
    changed: Arc<Notify>,
}

impl QuestionsAsked {
    /// The same grants, with `output` set as the picker of outputs' answer.
    pub fn answering_the_output_picker_with(output: &str) -> QuestionsAsked {
        QuestionsAsked {
            audio_output: Mutex::new(Some(output.to_owned())),
            ..QuestionsAsked::default()
        }
    }

    /// Sets the picker of outputs' answer and notifies waiters.
    pub fn set_audio_output_answer(&self, output: &str) {
        *self.audio_output.lock().unwrap() = Some(output.to_owned());
        self.changed.notify_waiters();
    }
}

impl ProgramGrants for QuestionsAsked {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: DomainRule::Only(Vec::new()),
        }
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
        self.answers.lock().unwrap().get(&question).copied()
    }

    fn set_answer(&self, question: Question, answer: Answer) {
        self.answers.lock().unwrap().insert(question, answer);
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

    fn audio_output_answer(&self) -> Option<String> {
        self.audio_output.lock().unwrap().clone()
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(Arc::clone(&self.changed).notified_owned())
    }
}
