//! The questions being put to a program's user now: a permission's, or whether it may open links
//! of a scheme, and the helper that races a waiting prompt against changes to the program's
//! settings. Each question in [`Asking`] is put once, however often the program asks while it is,
//! and every ask waits for the one answer.
//!
//! The invariant: a question is kept here only while it is being put. Its answer is kept, by
//! whoever asked, before the question leaves, so a program asking again once it has left reads the
//! answer rather than putting the question a second time.

use std::collections::HashMap;
use std::future::{Future, ready};
use std::hash::Hash;
use std::pin::pin;
use std::sync::{Arc, Mutex};

use futures::future::{BoxFuture, Either, FutureExt, Shared, select};
use waos_capabilities::{Answer, ChangedFuture, ProgramGrants};

use crate::access::{ProgramAccess, Prompt};
use crate::shell::AnswerFuture;
use crate::sync::locked;

/// A question being put: its answer, which every ask of it waits for; none when nobody answered.
pub(crate) type Asked = Shared<BoxFuture<'static, Option<Answer>>>;

/// What a waiting question resolved to: an answer from the prompt shown, or a stored answer
/// from the program's grants.
pub(crate) enum Outcome<P, S> {
    Prompt(P),
    Setting(S),
}

/// The questions being put now, by what each asks.
pub(crate) struct Asking<K>(Arc<Mutex<HashMap<K, Asked>>>);

impl<K> Default for Asking<K> {
    fn default() -> Asking<K> {
        Asking(Arc::default())
    }
}

impl<K: Clone + Eq + Hash + Send + 'static> Asking<K> {
    /// The answer to the question `key`: the one being put, or else `put`'s, put now.
    pub(crate) fn ask(&self, key: K, put: impl FnOnce() -> AnswerFuture) -> Asked {
        let mut asking = locked(&self.0);
        if let Some(asked) = asking.get(&key) {
            return asked.clone();
        }
        let asked = self.put(key.clone(), put()).boxed().shared();
        asking.insert(key, asked.clone());
        asked
    }

    /// `asked` answered, and the question no longer being put.
    fn put(
        &self,
        key: K,
        asked: AnswerFuture,
    ) -> impl Future<Output = Option<Answer>> + Send + 'static {
        let asking = Arc::clone(&self.0);
        async move {
            let answer = asked.await;
            locked(&asking).remove(&key);
            answer
        }
    }
}

/// Answers a question from `grants` if `read` finds an answer already, or puts `prompt` through
/// `access` (told as `prompt_kind`) and keeps a prompt answer with `keep`; if `grants` changes
/// while waiting so `read` finds an answer, `prompt` is dropped and that stored answer is
/// returned without calling `keep`.
pub(crate) fn ask_or_setting(
    access: &ProgramAccess,
    prompt_kind: Prompt,
    mut read: impl FnMut(&dyn ProgramGrants) -> Option<Answer> + Send + 'static,
    prompt: impl FnOnce() -> AnswerFuture,
    keep: impl FnOnce(&dyn ProgramGrants, Answer) + Send + 'static,
) -> AnswerFuture {
    let grants = access.clone_grants();
    let changed = grants.changed();
    if let Some(answer) = read(grants.as_ref()) {
        return Box::pin(ready(Some(answer)));
    }
    let prompt_future = prompt();
    let answered = async move {
        match or_setting(Arc::clone(&grants), changed, read, prompt_future).await {
            Outcome::Prompt(answer) => {
                if let Some(answer) = answer {
                    keep(grants.as_ref(), answer);
                }
                answer
            }
            Outcome::Setting(answer) => Some(answer),
        }
    };
    access.put_question(prompt_kind, Box::pin(answered))
}

/// Waits for `prompt`, or for `grants` to hold an answer `read` finds, whichever comes first.
///
/// `changed` must have been asked of `grants` before `read` last returned `None`, so any change
/// since then is heard. When `grants` changes and `read` returns `Some(answer)`, `prompt` is
/// dropped and `Outcome::Setting(answer)` is returned.
pub(crate) async fn or_setting<P, S, F: Future<Output = P>>(
    grants: Arc<dyn ProgramGrants>,
    mut changed: ChangedFuture,
    mut read: impl FnMut(&dyn ProgramGrants) -> Option<S>,
    prompt: F,
) -> Outcome<P, S> {
    let prompt = pin!(prompt);
    let wait_setting = pin!(async {
        loop {
            changed.await;
            changed = grants.changed();
            if let Some(answer) = read(grants.as_ref()) {
                return answer;
            }
        }
    });
    match select(wait_setting, prompt).await {
        Either::Left((set, _)) => Outcome::Setting(set),
        Either::Right((answered, _)) => Outcome::Prompt(answered),
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::channel::oneshot;
    use waos_capabilities::{DomainRule, FileSystemGrant, NetworkGrant, Question};

    use super::*;

    #[test]
    fn a_question_asked_twice_while_it_is_put_is_put_once_and_its_answer_kept_once() {
        let asking = Asking::default();
        let (answer, answered) = oneshot::channel();
        let mut answered = Some(answered);
        let kept = Arc::new(AtomicUsize::new(0));
        let mut ask = || {
            let kept = Arc::clone(&kept);
            let prompt = answered
                .take()
                .map(|answered| Box::pin(answered.map(|answer| answer.ok())) as AnswerFuture);
            asking.ask("zoommtg", || {
                let prompt = prompt.expect("put once");
                Box::pin(async move {
                    let answer = prompt.await;
                    if answer.is_some() {
                        kept.fetch_add(1, Ordering::SeqCst);
                    }
                    answer
                })
            })
        };
        let (first, second) = (ask(), ask());
        answer.send(Answer::Granted).unwrap();
        assert_eq!(first.now_or_never(), Some(Some(Answer::Granted)));
        assert_eq!(second.now_or_never(), Some(Some(Answer::Granted)));
        assert_eq!(kept.load(Ordering::SeqCst), 1);
        assert!(locked(&asking.0).is_empty(), "no longer being put");
    }

    struct LiveTestGrants {
        camera: Mutex<Option<Answer>>,
        kept: AtomicUsize,
        waiters: Mutex<Vec<oneshot::Sender<()>>>,
    }

    impl LiveTestGrants {
        fn new() -> Arc<LiveTestGrants> {
            Arc::new(LiveTestGrants {
                camera: Mutex::default(),
                kept: AtomicUsize::new(0),
                waiters: Mutex::default(),
            })
        }

        fn notify(&self) {
            for waiter in self.waiters.lock().unwrap().drain(..) {
                let _ = waiter.send(());
            }
        }

        fn set_camera(&self, answer: Answer) {
            *self.camera.lock().unwrap() = Some(answer);
            self.notify();
        }
    }

    impl ProgramGrants for LiveTestGrants {
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
            match question {
                Question::Camera => *self.camera.lock().unwrap(),
                _ => None,
            }
        }

        fn set_answer(&self, _question: Question, answer: Answer) {
            self.kept.fetch_add(1, Ordering::SeqCst);
            *self.camera.lock().unwrap() = Some(answer);
        }

        fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
            None
        }

        fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

        fn changed(&self) -> ChangedFuture {
            let (tx, rx) = oneshot::channel();
            self.waiters.lock().unwrap().push(tx);
            Box::pin(rx.map(|_| ()))
        }
    }

    #[test]
    fn a_waiting_question_answered_by_a_setting_drops_its_prompt_and_does_not_call_keep() {
        let grants = LiveTestGrants::new();
        let access = ProgramAccess::new(grants.clone());
        let (prompt_tx, prompt_rx) = oneshot::channel::<Option<Answer>>();
        let mut asked = ask_or_setting(
            &access,
            Prompt::Question(Question::Camera),
            |grants| grants.answer(Question::Camera),
            || Box::pin(prompt_rx.map(|answer| answer.unwrap_or(None))),
            |grants, answer| grants.set_answer(Question::Camera, answer),
        );
        assert_eq!((&mut asked).now_or_never(), None);
        assert!(!prompt_tx.is_canceled());

        // An unrelated change leaves the question waiting and its prompt intact.
        grants.notify();
        assert_eq!((&mut asked).now_or_never(), None);
        assert!(!prompt_tx.is_canceled());

        // Setting the answer finishes the question, drops the prompt future, and skips keep.
        grants.set_camera(Answer::Granted);
        assert_eq!((&mut asked).now_or_never(), Some(Some(Answer::Granted)));
        assert!(prompt_tx.is_canceled(), "prompt future was dropped");
        assert_eq!(grants.kept.load(Ordering::SeqCst), 0);

        // And a fresh ask reads the stored answer without calling prompt at all.
        let mut second = ask_or_setting(
            &access,
            Prompt::Question(Question::Camera),
            |grants| grants.answer(Question::Camera),
            || {
                panic!("stored answer means don't ask");
                #[allow(unreachable_code)]
                Box::pin(pending())
            },
            |grants, answer| grants.set_answer(Question::Camera, answer),
        );
        assert_eq!((&mut second).now_or_never(), Some(Some(Answer::Granted)));
    }
}
