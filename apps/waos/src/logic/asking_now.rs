//! What WaOS asks each program's user now, by the program's name: a question, a link scheme, a
//! switch's page, a picker or a file dialog, from the moment it is asked until it is answered or
//! dropped. A run of `cargo waos run` hears each as it is asked, and says it on its terminal; a
//! screenshot and an input read what is asked over the program's windows, which no tool acts
//! through (the dev server's `dev_run`, `dev_screenshot` and `dev_input`).
//!
//! The invariant: what is noted goes when its answer comes or its asking is dropped, by the guard
//! held in the future that waits for it, so nothing is said to be asked that no longer is.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::broadcast;
use waos::RunningProgram;
use waos_dev_protocol::Asked;

use super::setting_changes::SettingChange;

/// How many askings a run that is slow to read may fall behind by before it misses some.
const TOLD_BACKLOG: usize = 64;

/// A setting `cargo waos set` made while a program's questions wait, or a cancel, for the main
/// thread to answer the question with: the change the set resolved and checked, or the question
/// whose prompt is to be left unanswered.
#[derive(Clone, Debug)]
pub(crate) enum AnswerNow {
    Set {
        program: String,
        change: SettingChange,
    },
    Cancel {
        program: String,
        asked: Asked,
    },
}

/// What WaOS asks each program's user now. Every clone of the `Arc` is the same.
pub(crate) struct AskingNow {
    /// Each program's askings, by its name, in the order asked, each with its serial.
    asked: Mutex<HashMap<String, Vec<(u64, Asked)>>>,
    serials: AtomicU64,
    /// Each asking as it starts, with its program's name.
    told: broadcast::Sender<(String, Asked)>,
    /// Each setting made while a question waits, for whoever answers it now.
    answers: broadcast::Sender<AnswerNow>,
}

/// One asking noted, until dropped.
pub(crate) struct Noted {
    asking: Arc<AskingNow>,
    program: String,
    serial: u64,
}

impl AskingNow {
    pub(crate) fn new() -> Arc<AskingNow> {
        Arc::new(AskingNow {
            asked: Mutex::default(),
            serials: AtomicU64::new(0),
            told: broadcast::channel(TOLD_BACKLOG).0,
            answers: broadcast::channel(TOLD_BACKLOG).0,
        })
    }

    /// Each setting made while a question waits, from now on, for the main thread to answer it.
    pub(crate) fn answers(&self) -> broadcast::Receiver<AnswerNow> {
        self.answers.subscribe()
    }

    /// A change was made for the program `program`: a question it answers, waiting now, is
    /// answered with it.
    pub(crate) fn answer(&self, program: &str, change: SettingChange) {
        // Nobody answering means the next question is answered from the setting, as before.
        let _ = self.answers.send(AnswerNow::Set {
            program: program.to_owned(),
            change,
        });
    }

    /// The question `asked` names, waiting now, is left unanswered, as a user cancels it.
    pub(crate) fn cancel(&self, program: &str, asked: Asked) {
        let _ = self.answers.send(AnswerNow::Cancel {
            program: program.to_owned(),
            asked,
        });
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

    /// What the program `name` names is asked first of what is asked now; none while nothing is.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) fn of(&self, name: &str) -> Option<Asked> {
        let asked = self.asked.lock().unwrap_or_else(PoisonError::into_inner);
        asked.get(name)?.first().map(|(_, asked)| asked.clone())
    }

    /// Each asking from now on as it starts, with its program's name.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) fn told(&self) -> broadcast::Receiver<(String, Asked)> {
        self.told.subscribe()
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
        let asking = AskingNow::new();
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
            told.try_recv().unwrap(),
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
        let asking = AskingNow::new();
        let (answer, answered) = oneshot::channel::<bool>();
        let waiting = asking.while_asked_of("dev.example.terminal", Asked::Machine, answered);
        assert_eq!(asking.of("dev.example.terminal"), Some(Asked::Machine));
        answer.send(true).unwrap();
        assert_eq!(block_on(waiting), Ok(true));
        assert_eq!(asking.of("dev.example.terminal"), None);
    }
}
