//! The questions being put to a program's user now: a permission's, or whether it may open links
//! of a scheme, and the helper that races a waiting prompt against changes to the program's
//! settings. Each question in [`Asking`] is put once, however often the program asks while it is,
//! and every ask waits for the one answer.
//!
//! The invariant: a question is kept here only while it is being put. Its answer is kept, by
//! whoever asked, before the question leaves, so a program asking again once it has left reads the
//! answer rather than putting the question a second time.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures::future::{BoxFuture, Either, FutureExt, Shared, select};
use waos_capabilities::{Answer, ProgramGrants};

use crate::access::{Happened, ProgramAccess, Prompt};
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
pub(crate) async fn ask_or_setting(
    access: &ProgramAccess,
    prompt_kind: Prompt,
    read: impl FnMut(&dyn ProgramGrants) -> Option<Answer>,
    prompt: impl FnOnce() -> AnswerFuture,
    keep: impl FnOnce(&dyn ProgramGrants, Answer),
) -> Option<Answer> {
    let grants = access.clone_grants();
    let asked = AtomicBool::new(false);
    let outcome = stored_or_shown(Arc::clone(&grants), read, || {
        asked.store(true, Ordering::Relaxed);
        access.put_question(prompt_kind.clone(), prompt())
    })
    .await;
    match outcome {
        Outcome::Prompt(answer) => {
            if let Some(answer) = answer {
                keep(grants.as_ref(), answer);
            }
            answer
        }
        Outcome::Setting(answer) => {
            if asked.load(Ordering::Relaxed) {
                access.tell(Happened::QuestionAnswered(prompt_kind, Some(answer)));
            }
            Some(answer)
        }
    }
}

/// Resolves once `grants` holds an answer `read` finds, registering `grants.changed()` before
/// each read so no change between a `None` read and waiting can be missed.
async fn settled<S>(
    grants: Arc<dyn ProgramGrants>,
    mut read: impl FnMut(&dyn ProgramGrants) -> Option<S>,
) -> S {
    loop {
        let changed = grants.changed();
        if let Some(answer) = read(&*grants) {
            return answer;
        }
        changed.await;
    }
}

/// Answers what `read` already finds in `grants` without calling `show`, or else calls `show` and
/// races it against `grants` changing to an answer `read` finds, dropping `show`'s future if a
/// stored answer wins first.
pub(crate) async fn stored_or_shown<P, S, F: Future<Output = P>>(
    grants: Arc<dyn ProgramGrants>,
    read: impl FnMut(&dyn ProgramGrants) -> Option<S>,
    show: impl FnOnce() -> F,
) -> Outcome<P, S> {
    let mut setting = pin!(settled(grants, read));
    if let Some(answer) = setting.as_mut().now_or_never() {
        return Outcome::Setting(answer);
    }
    let shown = pin!(show());
    match select(setting, shown).await {
        Either::Left((answer, _)) => Outcome::Setting(answer),
        Either::Right((answered, _)) => Outcome::Prompt(answered),
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::channel::oneshot;
    use tokio::sync::Notify;
    use waos_capabilities::{ChangedFuture, DomainRule, FileSystemGrant, NetworkGrant, Question};

    use super::*;

    #[test]
    fn a_question_asked_twice_while_it_is_put_is_put_once_and_its_answer_kept_once() {
        let grants = LiveTestGrants::new();
        let access = ProgramAccess::new(grants.clone());
        let asking = Asking::default();
        let (answer, answered) = oneshot::channel();
        let mut answered = Some(answered);
        let mut ask = || {
            let access = access.clone();
            let prompt = answered
                .take()
                .map(|answered| Box::pin(answered.map(|answer| answer.ok())) as AnswerFuture);
            asking.ask(Question::Camera, || {
                Box::pin(async move {
                    ask_or_setting(
                        &access,
                        Prompt::Question(Question::Camera),
                        |grants| grants.answer(Question::Camera),
                        || prompt.expect("put once"),
                        |grants, answer| grants.set_answer(Question::Camera, answer),
                    )
                    .await
                })
            })
        };
        let (first, second) = (ask(), ask());
        answer.send(Answer::Granted).unwrap();
        assert_eq!(first.now_or_never(), Some(Some(Answer::Granted)));
        assert_eq!(second.now_or_never(), Some(Some(Answer::Granted)));
        assert_eq!(grants.kept.load(Ordering::SeqCst), 1);
        assert!(locked(&asking.0).is_empty(), "no longer being put");
    }

    struct LiveTestGrants {
        camera: Mutex<Option<Answer>>,
        kept: AtomicUsize,
        notify: Arc<Notify>,
    }

    impl LiveTestGrants {
        fn new() -> Arc<LiveTestGrants> {
            Arc::new(LiveTestGrants {
                camera: Mutex::default(),
                kept: AtomicUsize::new(0),
                notify: Arc::new(Notify::new()),
            })
        }

        fn set_camera(&self, answer: Answer) {
            *self.camera.lock().unwrap() = Some(answer);
            self.notify.notify_waiters();
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
            Box::pin(Arc::clone(&self.notify).notified_owned())
        }
    }

    #[test]
    fn a_waiting_question_answered_by_a_setting_drops_its_prompt_and_does_not_call_keep() {
        let grants = LiveTestGrants::new();
        let access = ProgramAccess::new(grants.clone());
        let (prompt_tx, prompt_rx) = oneshot::channel::<Option<Answer>>();
        let mut asked = Box::pin(ask_or_setting(
            &access,
            Prompt::Question(Question::Camera),
            |grants| grants.answer(Question::Camera),
            || Box::pin(prompt_rx.map(|answer| answer.unwrap_or(None))),
            |grants, answer| grants.set_answer(Question::Camera, answer),
        ));
        assert_eq!((&mut asked).now_or_never(), None);
        assert!(!prompt_tx.is_canceled());

        // An unrelated change leaves the question waiting and its prompt intact.
        grants.notify.notify_waiters();
        assert_eq!((&mut asked).now_or_never(), None);
        assert!(!prompt_tx.is_canceled());

        // Setting the answer finishes the question, drops the prompt future, and skips keep.
        grants.set_camera(Answer::Granted);
        assert_eq!((&mut asked).now_or_never(), Some(Some(Answer::Granted)));
        assert!(prompt_tx.is_canceled(), "prompt future was dropped");
        assert_eq!(grants.kept.load(Ordering::SeqCst), 0);

        // And a fresh ask reads the stored answer without calling prompt at all.
        let mut second = Box::pin(ask_or_setting(
            &access,
            Prompt::Question(Question::Camera),
            |grants| grants.answer(Question::Camera),
            || {
                panic!("stored answer means don't ask");
                #[allow(unreachable_code)]
                Box::pin(pending())
            },
            |grants, answer| grants.set_answer(Question::Camera, answer),
        ));
        assert_eq!((&mut second).now_or_never(), Some(Some(Answer::Granted)));
    }
}
