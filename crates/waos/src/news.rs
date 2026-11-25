//! What the platform tells a program, each item heard by every stream of it the program opened:
//! the choices made in a menu and what the user does to a tray icon ([`News`]); and what must
//! reach the program even when it is told before the program listens, a request to open files or
//! links ([`KeptNews`]), which waits for the program's next stream.
//!
//! The invariant: an item on its way to a program holds it unfrozen until the program has handled
//! it on each stream that carries it, read it and come back for the next or dropped the stream, so
//! a frozen program answers what it is told as it answers a paste of a form it promised; and only
//! an item a stream will carry holds it, so a program that reads nothing is not unfrozen for
//! nothing, and an item kept for a stream not yet open holds nothing until one opens. A stream of
//! requests that each need one answer has one reader instead (`clipboard`'s source's).

use std::sync::{Arc, Mutex};

use futures::channel::mpsc;

use crate::channel::Channel;
use crate::lifecycle::{FreezeInhibitor, FreezeInhibitorKind, Lifecycle};
use crate::sync::locked;

/// An item on its way to the program, with what holds the program unfrozen until it has handled
/// it.
pub(crate) type Inhibiting<T> = (T, Arc<FreezeInhibitor>);

/// An item on its way to the program, with what holds the program unfrozen until it has handled it
/// where the item decides whether the program goes on: a view's going, a request to close one.
pub(crate) type MaybeInhibiting<T> = (T, Option<Arc<FreezeInhibitor>>);

/// News: each item told heard by every stream open.
pub(crate) struct News<T>(Channel<Inhibiting<T>>);

impl<T> Default for News<T> {
    fn default() -> News<T> {
        News(Channel::default())
    }
}

impl<T: Clone> News<T> {
    /// Tells `item`: every stream open hears it, the program, whose lifecycle is `lifecycle`,
    /// unfrozen until it has handled it on each.
    pub(crate) fn tell(&self, item: T, lifecycle: &Lifecycle) {
        if self.0.is_heard() {
            self.0.send((
                item,
                Arc::new(lifecycle.inhibit_freeze(FreezeInhibitorKind::PendingRequest)),
            ));
        }
    }

    /// What is told from now on, on a stream of its own.
    pub(crate) fn subscribe(&self) -> mpsc::UnboundedReceiver<Inhibiting<T>> {
        self.0.subscribe()
    }

    /// Whether a stream is open to hear what is told now.
    fn is_heard(&self) -> bool {
        self.0.is_heard()
    }
}

/// News kept until heard: what is told while no stream is open waits, and the next stream the
/// program opens hears it first, so a program started to open something hears it once it listens.
pub(crate) struct KeptNews<T> {
    news: News<T>,
    /// What was told while no stream was open, in the order told.
    waiting: Mutex<Vec<T>>,
}

impl<T> Default for KeptNews<T> {
    fn default() -> KeptNews<T> {
        KeptNews {
            news: News::default(),
            waiting: Mutex::default(),
        }
    }
}

impl<T: Clone> KeptNews<T> {
    /// Tells `item`: every stream open hears it, the program, whose lifecycle is `lifecycle`,
    /// unfrozen until it has handled it on each; while none is open, it waits for the next.
    pub(crate) fn tell(&self, item: T, lifecycle: &Lifecycle) {
        let mut waiting = locked(&self.waiting);
        if self.news.is_heard() {
            self.news.tell(item, lifecycle);
        } else {
            waiting.push(item);
        }
    }

    /// What is told from now on, on a stream of its own, after what waited for it, which unfreezes
    /// the program, whose lifecycle is `lifecycle`, as it would have when told.
    pub(crate) fn subscribe(
        &self,
        lifecycle: &Lifecycle,
    ) -> mpsc::UnboundedReceiver<Inhibiting<T>> {
        let mut waiting = locked(&self.waiting);
        let stream = self.news.subscribe();
        for item in waiting.drain(..) {
            self.news.tell(item, lifecycle);
        }
        stream
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_news_told_while_nobody_listens_reaches_the_next_stream_alone() {
        let lifecycle = Lifecycle::default();
        let news = KeptNews::default();
        news.tell("first", &lifecycle);
        news.tell("second", &lifecycle);
        let mut stream = news.subscribe(&lifecycle);
        let mut later = news.subscribe(&lifecycle);
        news.tell("third", &lifecycle);
        let heard: Vec<_> = std::iter::from_fn(|| stream.try_recv().ok())
            .map(|(item, _unfreeze)| item)
            .collect();
        assert_eq!(heard, ["first", "second", "third"]);
        let (item, _unfreeze) = later.try_recv().unwrap();
        assert_eq!(item, "third", "what waited went to the first stream");
    }

    #[test]
    fn kept_news_unfreezes_a_frozen_program_only_once_a_stream_carries_it() {
        let lifecycle = Lifecycle::default();
        lifecycle.app_suspending();
        let news = KeptNews::default();
        news.tell("opened", &lifecycle);
        assert!(lifecycle.is_frozen(), "nothing carries it yet");
        let mut stream = news.subscribe(&lifecycle);
        assert!(!lifecycle.is_frozen(), "unfrozen while it is carried");
        drop(stream.try_recv().unwrap());
        assert!(lifecycle.is_frozen(), "handled, the rule applies again");
    }
}
