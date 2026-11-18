//! Grants for a program whose user is asked at the first use: nothing switched on, the answers to
//! the questions put kept as the store keeps them, none answered yet, every link scheme refused,
//! and the picker of outputs asked unless a test set its answer.

use std::collections::HashMap;
use std::sync::Mutex;

use futures::FutureExt;
use futures::channel::oneshot;
use waos::{Answer, ProgramGrants, Question};
use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant, SimulatedSound};

/// Grants that switch nothing on and keep the answers to the questions put, none answered yet.
#[derive(Default)]
pub struct QuestionsAsked {
    answers: Mutex<HashMap<Question, Answer>>,
    /// The output, by its id, set as the picker of outputs' answer.
    audio_output: Mutex<Option<String>>,
    /// What a simulated microphone records.
    simulated_microphone: SimulatedSound,
    waiting: Mutex<Vec<oneshot::Sender<()>>>,
}

impl QuestionsAsked {
    /// Grants whose microphone is simulated, recording silence.
    pub fn silent_simulated_microphone() -> QuestionsAsked {
        let grants = QuestionsAsked {
            simulated_microphone: SimulatedSound::Silence,
            ..QuestionsAsked::default()
        };
        grants.set_answer(Question::Microphone, Answer::Simulated);
        grants
    }

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
        for waiter in self.waiting.lock().unwrap().drain(..) {
            let _ = waiter.send(());
        }
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

    fn simulated_microphone(&self) -> SimulatedSound {
        self.simulated_microphone
    }

    fn changed(&self) -> ChangedFuture {
        let (tx, rx) = oneshot::channel();
        self.waiting.lock().unwrap().push(tx);
        Box::pin(rx.map(|_| ()))
    }
}
