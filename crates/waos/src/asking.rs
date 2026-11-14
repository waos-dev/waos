//! The questions being put to a program's user now: a permission's, or whether it may open links
//! of a scheme. Each is put once, however often the program asks while it is, and every ask waits
//! for the one answer.
//!
//! The invariant: a question is kept here only while it is being put. Its answer is kept, by
//! whoever asked, before the question leaves, so a program asking again once it has left reads the
//! answer rather than putting the question a second time.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use futures::future::{BoxFuture, FutureExt, Shared};
use waos_capabilities::Answer;

use crate::shell::AnswerFuture;
use crate::sync::locked;

/// A question being put: its answer, which every ask of it waits for; none when nobody answered.
pub(crate) type Asked = Shared<BoxFuture<'static, Option<Answer>>>;

/// The questions being put now, by what each asks.
pub(crate) struct Asking<K>(Arc<Mutex<HashMap<K, Asked>>>);

impl<K> Default for Asking<K> {
    fn default() -> Asking<K> {
        Asking(Arc::default())
    }
}

impl<K: Clone + Eq + Hash + Send + 'static> Asking<K> {
    /// The answer to the question `key`: the one being put, or else `prompt`'s, put now, which
    /// `keep` keeps once it comes.
    pub(crate) fn ask(
        &self,
        key: K,
        prompt: impl FnOnce() -> AnswerFuture,
        keep: impl FnOnce(Answer) + Send + 'static,
    ) -> Asked {
        let mut asking = locked(&self.0);
        if let Some(asked) = asking.get(&key) {
            return asked.clone();
        }
        let asked = self.put(key.clone(), prompt(), keep).boxed().shared();
        asking.insert(key, asked.clone());
        asked
    }

    /// `prompt` answered, the answer kept, and the question no longer being put.
    fn put(
        &self,
        key: K,
        prompt: AnswerFuture,
        keep: impl FnOnce(Answer) + Send + 'static,
    ) -> impl Future<Output = Option<Answer>> + Send + 'static {
        let asking = Arc::clone(&self.0);
        async move {
            let answer = prompt.await;
            if let Some(answer) = answer {
                keep(answer);
            }
            locked(&asking).remove(&key);
            answer
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::channel::oneshot;

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
            asking.ask(
                "zoommtg",
                || prompt.expect("put once"),
                move |_answer| {
                    kept.fetch_add(1, Ordering::SeqCst);
                },
            )
        };
        let (first, second) = (ask(), ask());
        answer.send(Answer::Granted).unwrap();
        assert_eq!(first.now_or_never(), Some(Some(Answer::Granted)));
        assert_eq!(second.now_or_never(), Some(Some(Answer::Granted)));
        assert_eq!(kept.load(Ordering::SeqCst), 1);
        assert!(locked(&asking.0).is_empty(), "no longer being put");
    }
}
