//! What WaOS asks each program's user now, by the program's name: what Permissions asks, its one
//! list of the questions being asked (a question, a link scheme, a role, a switch's page), and
//! what the shell's pickers and file dialogs ask, noted here, from the moment it is asked until it
//! is answered or dropped. An attach hears each as it is asked, and what is asked as it attaches,
//! and `waos attach` and `waos run` say it on their terminal; a screenshot and an input read what
//! is asked over the program's windows, which no tool acts through (the dev server's
//! `dev_attach`, `dev_screenshot` and `dev_input`).
//!
//! The invariant: what is noted goes when its answer comes or its asking is dropped, by the guard
//! held in the future that waits for it, so nothing is said to be asked that no longer is; and a
//! question of Permissions' is read from its list, never noted here a second time.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::{broadcast, watch};
use waos::RunningProgram;
#[cfg(desktop)]
use waos::{Question, Switch};
use waos_dev_protocol::Asked;
use waos_services::permissions::AskedQuestion;
#[cfg(desktop)]
use waos_services::permissions::{AppQuestion, Role};

/// How many askings a run that is slow to read may fall behind by before it misses some.
const TOLD_BACKLOG: usize = 64;

/// What WaOS asks each program's user now. Every clone of the `Arc` is the same.
pub(crate) struct AskingNow {
    /// What Permissions asks now, which only the dev tools read.
    #[cfg_attr(
        not(desktop),
        expect(dead_code, reason = "a phone has no dev tools yet")
    )]
    questions: watch::Receiver<Vec<AskedQuestion>>,
    /// Each program's pickers and dialogs, by its name, in the order asked, each with its serial.
    asked: Mutex<HashMap<String, Vec<(u64, Asked)>>>,
    serials: AtomicU64,
    /// Each picker or dialog as it starts, with its program's name.
    told: broadcast::Sender<(String, Asked)>,
}

/// What an attach hears of what is asked from now on: Permissions' questions as they join its
/// list, and the pickers and dialogs as they start.
#[cfg(desktop)]
pub(crate) struct Told {
    questions: watch::Receiver<Vec<AskedQuestion>>,
    /// Permissions' questions as last heard.
    heard: Vec<AskedQuestion>,
    noted: broadcast::Receiver<(String, Asked)>,
}

/// One asking noted, until dropped.
pub(crate) struct Noted {
    asking: Arc<AskingNow>,
    program: String,
    serial: u64,
}

impl AskingNow {
    /// What is asked now: `questions`, Permissions' list, and the pickers and dialogs noted here.
    pub(crate) fn new(questions: watch::Receiver<Vec<AskedQuestion>>) -> Arc<AskingNow> {
        Arc::new(AskingNow {
            questions,
            asked: Mutex::default(),
            serials: AtomicU64::new(0),
            told: broadcast::channel(TOLD_BACKLOG).0,
        })
    }

    /// Waits for `answer`, which `program`'s user gives to `asked`, noting it as asked meanwhile.
    pub(crate) fn while_asked<T>(
        self: &Arc<AskingNow>,
        program: &RunningProgram,
        asked: Asked,
        answer: impl Future<Output = T> + Send + 'static,
    ) -> impl Future<Output = T> + Send + 'static {
        self.while_asked_of(program.name(), asked, answer)
    }

    /// [`while_asked`](Self::while_asked), of the program named `name`.
    fn while_asked_of<T>(
        self: &Arc<AskingNow>,
        name: &str,
        asked: Asked,
        answer: impl Future<Output = T> + Send + 'static,
    ) -> impl Future<Output = T> + Send + 'static {
        let noted = self.note(name, asked);
        async move {
            let answered = answer.await;
            drop(noted);
            answered
        }
    }

    /// What the program `name` names is asked first of what is asked now, Permissions' questions
    /// before the pickers and dialogs; none while nothing is.
    #[cfg(desktop)]
    pub(crate) fn of(&self, name: &str) -> Option<Asked> {
        let question = (self.questions.borrow().iter())
            .find(|asked| asked.app.as_str() == name)
            .map(|asked| protocol_asked(&asked.question));
        if question.is_some() {
            return question;
        }
        let asked = self.asked.lock().unwrap_or_else(PoisonError::into_inner);
        asked.get(name)?.first().map(|(_, asked)| asked.clone())
    }

    /// Each asking from now on as it starts, with its program's name.
    #[cfg(desktop)]
    pub(crate) fn told(&self) -> Told {
        let mut questions = self.questions.clone();
        let heard = questions.borrow_and_update().clone();
        Told {
            questions,
            heard,
            noted: self.told.subscribe(),
        }
    }

    fn note(self: &Arc<AskingNow>, name: &str, asked: Asked) -> Noted {
        let serial = self.serials.fetch_add(1, Ordering::Relaxed);
        let mut noted = self.asked.lock().unwrap_or_else(PoisonError::into_inner);
        noted
            .entry(name.to_owned())
            .or_default()
            .push((serial, asked.clone()));
        drop(noted);
        // Nobody listening is nobody to tell.
        let _ = self.told.send((name.to_owned(), asked));
        Noted {
            asking: Arc::clone(self),
            program: name.to_owned(),
            serial,
        }
    }
}

#[cfg(desktop)]
impl Told {
    /// The next asking to start, with its program's name; lagged when some were missed, and
    /// closed once nothing more can be asked.
    pub(crate) async fn next(&mut self) -> Result<(String, Asked), broadcast::error::RecvError> {
        loop {
            let now = self.questions.borrow_and_update().clone();
            let new = now
                .iter()
                .find(|asked| !self.heard.contains(asked))
                .cloned();
            self.heard = now;
            if let Some(new) = new {
                return Ok((new.app.to_string(), protocol_asked(&new.question)));
            }
            tokio::select! {
                noted = self.noted.recv() => return noted,
                changed = self.questions.changed() => {
                    if changed.is_err() {
                        return self.noted.recv().await;
                    }
                }
            }
        }
    }
}

/// A question of Permissions', as the dev protocol says it.
#[cfg(desktop)]
fn protocol_asked(question: &AppQuestion) -> Asked {
    match question {
        AppQuestion::Permission(question) => Asked::Question(protocol_question(*question)),
        AppQuestion::LinkScheme(scheme) => Asked::LinkScheme(scheme.clone()),
        AppQuestion::Role(Role::Inspector) => Asked::Role(waos_dev_protocol::Role::Inspector),
        AppQuestion::Switch(switch) => Asked::Switch(protocol_switch(*switch)),
    }
}

#[cfg(desktop)]
fn protocol_question(question: Question) -> waos_dev_protocol::Question {
    match question {
        Question::Notifications => waos_dev_protocol::Question::Notifications,
        Question::Camera => waos_dev_protocol::Question::Camera,
        Question::Microphone => waos_dev_protocol::Question::Microphone,
    }
}

#[cfg(desktop)]
fn protocol_switch(switch: Switch) -> waos_dev_protocol::Switch {
    match switch {
        Switch::Network => waos_dev_protocol::Switch::Network,
        Switch::LocalNetwork => waos_dev_protocol::Switch::LocalNetwork,
        Switch::ClipboardRead => waos_dev_protocol::Switch::ClipboardRead,
        Switch::ClipboardWrite => waos_dev_protocol::Switch::ClipboardWrite,
    }
}

impl Drop for Noted {
    fn drop(&mut self) {
        let mut asked = self
            .asking
            .asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(askings) = asked.get_mut(&self.program) {
            askings.retain(|(serial, _)| *serial != self.serial);
            if askings.is_empty() {
                asked.remove(&self.program);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::channel::oneshot;
    use futures::executor::block_on;
    use waos_dev_protocol::{FileDialog, Question};

    use super::*;

    fn noted(asking: &Arc<AskingNow>, name: &str, asked: Asked) -> Noted {
        asking.note(name, asked)
    }

    #[test]
    fn what_is_asked_is_told_as_it_starts_and_goes_as_it_is_answered() {
        let asking = AskingNow::new(watch::channel(Vec::new()).1);
        let mut told = asking.told();
        let camera = noted(
            &asking,
            "dev.example.camera",
            Asked::Question(Question::Camera),
        );
        let saving = noted(
            &asking,
            "dev.example.camera",
            Asked::FileDialog(FileDialog::SaveFile),
        );
        assert_eq!(
            block_on(told.next()).unwrap(),
            (
                "dev.example.camera".to_owned(),
                Asked::Question(Question::Camera)
            )
        );
        assert_eq!(
            asking.of("dev.example.camera"),
            Some(Asked::Question(Question::Camera))
        );
        assert_eq!(asking.of("dev.example.reader"), None);
        drop(camera);
        assert_eq!(
            asking.of("dev.example.camera"),
            Some(Asked::FileDialog(FileDialog::SaveFile)),
            "the next one asked"
        );
        drop(saving);
        assert_eq!(asking.of("dev.example.camera"), None);
    }

    #[test]
    fn an_asking_waited_for_is_noted_until_its_answer_comes() {
        let asking = AskingNow::new(watch::channel(Vec::new()).1);
        let (answer, answered) = oneshot::channel::<bool>();
        let waiting = asking.while_asked_of("dev.example.terminal", Asked::Machine, answered);
        assert_eq!(asking.of("dev.example.terminal"), Some(Asked::Machine));
        answer.send(true).unwrap();
        assert_eq!(block_on(waiting), Ok(true));
        assert_eq!(asking.of("dev.example.terminal"), None);
    }
}
