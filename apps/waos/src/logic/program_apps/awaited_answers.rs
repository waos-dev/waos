//! What WaOS asked of a program's app and awaits the answer to, as the program's call awaits it: a
//! context menu, which answers the item chosen, a file dialog, which answers the paths picked, a
//! notification's post or schedule, which answers whether the system has it, and macOS's question
//! for the program's notifications, which answers whether macOS refuses them.
//! Each is asked by an id of its own, which the window process's answer names back, and its answer
//! resolves the call once the app gives it, or says the app went first.
//!
//! The invariant a later editor will break: everything asked is answered exactly once, by the
//! app's answer or as the app goes, and nothing is asked once the app is gone, so no program waits
//! on what nobody shows.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};

use futures::FutureExt;
use futures::channel::oneshot;

/// The app went before it answered.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AppGone;

/// Why what was asked of an app that went answers nothing, as the program hears it.
pub(crate) const APP_GONE: &str = "the program's app is gone";

/// The answers awaited from one program's app, each a `T`.
pub(crate) struct AwaitedAnswers<T> {
    awaited: Arc<Mutex<Awaited<T>>>,
}

struct Awaited<T> {
    last: u64,
    /// Where each answer awaited resolves the program's call.
    waiting: HashMap<u64, oneshot::Sender<T>>,
    /// Whether the app is gone, after which nothing is asked.
    gone: bool,
}

/// One answer awaited from the app: the app's, or [`AppGone`] once it went first. Dropping it
/// removes the entry from what is awaited so a dropped call never keeps the app owing answers.
pub(crate) struct AwaitedAnswer<T> {
    asked: u64,
    awaited: Arc<Mutex<Awaited<T>>>,
    answered: oneshot::Receiver<T>,
}

impl<T> AwaitedAnswer<T> {
    /// Stops awaiting the answer now; answers whether the app had not answered or gone first.
    pub(crate) fn cancel(&self) -> bool {
        self.awaited
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .waiting
            .remove(&self.asked)
            .is_some()
    }
}

impl<T> Drop for AwaitedAnswer<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl<T> AwaitedAnswers<T> {
    /// Something about to be asked of the app: the id the app's answer names, and the answer; none
    /// once the app is gone.
    pub(crate) fn ask(&self) -> Option<(u64, AwaitedAnswer<T>)> {
        let mut awaited = self.awaited();
        if awaited.gone {
            return None;
        }
        awaited.last += 1;
        let asked = awaited.last;
        let (answer, answered) = oneshot::channel();
        awaited.waiting.insert(asked, answer);
        Some((
            asked,
            AwaitedAnswer {
                asked,
                awaited: Arc::clone(&self.awaited),
                answered,
            },
        ))
    }

    /// The app answered `answer` to what `asked` names.
    pub(crate) fn answered(&self, asked: u64, answer: T) {
        if let Some(waiting) = self.awaited().waiting.remove(&asked) {
            // A program that ended meanwhile hears nothing.
            let _ = waiting.send(answer);
        }
    }

    /// Whether an answer is still awaited.
    pub(crate) fn any(&self) -> bool {
        !self.awaited().waiting.is_empty()
    }

    /// The app is gone: everything still awaited hears so, and nothing is asked from now on.
    pub(crate) fn app_gone(&self) {
        let mut awaited = self.awaited();
        awaited.gone = true;
        awaited.waiting.clear();
    }

    fn awaited(&self) -> MutexGuard<'_, Awaited<T>> {
        self.awaited.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T> Default for AwaitedAnswers<T> {
    fn default() -> AwaitedAnswers<T> {
        AwaitedAnswers {
            awaited: Arc::new(Mutex::new(Awaited {
                last: 0,
                waiting: HashMap::new(),
                gone: false,
            })),
        }
    }
}

impl<T> Future for AwaitedAnswer<T> {
    type Output = Result<T, AppGone>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.answered
            .poll_unpin(cx)
            .map(|answer| answer.map_err(|_| AppGone))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_answer_resolves_what_it_names() {
        let awaited = AwaitedAnswers::default();
        let (first, first_answer) = awaited.ask().expect("asked");
        let (second, second_answer) = awaited.ask().expect("asked again");
        assert_ne!(first, second);
        awaited.answered(second, 40);
        assert_eq!(second_answer.now_or_never(), Some(Ok(40)));
        assert_eq!(first_answer.now_or_never(), None, "its answer has not come");
    }

    #[test]
    fn once_the_app_is_gone_everything_awaited_hears_so_and_nothing_is_asked() {
        let awaited = AwaitedAnswers::default();
        let (asked, answer) = awaited.ask().expect("asked");
        awaited.app_gone();
        assert_eq!(answer.now_or_never(), Some(Err(AppGone)));
        awaited.answered(asked, 40);
        assert!(
            awaited.ask().is_none(),
            "nothing is asked of an app that is gone"
        );
    }

    #[test]
    fn dropping_an_awaited_answer_stops_awaiting_it() {
        let awaited = AwaitedAnswers::<u32>::default();
        let (_id, answer) = awaited.ask().expect("asked");
        assert!(awaited.any());
        drop(answer);
        assert!(!awaited.any());
    }
}
