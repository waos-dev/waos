//! Where the framework asks to be woken, and the loop's timer over what it asked for.
//!
//! Waft reaches a [`Dispatcher`] from any thread, because the asking happens inside a task's
//! waker and Rust requires a waker, and everything it captures, to be sendable whether or not
//! the target has threads. A program has one thread and one loop, so [`Wake`] carries only what
//! is sendable: the timer wakeup the framework sets whole, a flag for a ready task asking to be
//! woken at once, and the loop's notices, which is how that flag reaches a loop already asleep on
//! its streams.
//!
//! The two are kept apart on purpose. A timer wakeup is a time and stays set until it arrives or
//! the framework moves it; a wake asked for by a ready task is spent by the next wake and never
//! touches the timer. [`Alarm`] is the loop's side of the first: one wait on the program's
//! monotonic clock, made again whenever the wakeup moves: earlier, later or away.

use std::future::{self, Future};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use futures::channel::mpsc;
use waft_embedder::{Dispatcher, Instant};
use wasdk::monotonic_clock;

use crate::events::Notice;

/// What the framework has asked the loop for.
pub(crate) struct Wake {
    timer_wakeup: Mutex<Option<Instant>>,
    now_requested: AtomicBool,
    /// Carries a `wake_now` to the loop, which may be asleep on its streams.
    notices: mpsc::UnboundedSender<Notice>,
}

impl Wake {
    pub(crate) fn new(notices: mpsc::UnboundedSender<Notice>) -> Wake {
        Wake {
            timer_wakeup: Mutex::new(None),
            now_requested: AtomicBool::new(false),
            notices,
        }
    }

    /// When the loop should next wake for the framework's earliest waiting timer.
    pub(crate) fn timer_wakeup(&self) -> Option<Instant> {
        *self.lock()
    }

    /// The timer wakeup has arrived. It is cleared unless the framework has already moved it
    /// past `now`, in which case that later one is still waiting.
    pub(crate) fn timer_arrived(&self, now: Instant) {
        let mut wakeup = self.lock();
        if wakeup.is_some_and(|at| at <= now) {
            *wakeup = None;
        }
    }

    /// The loop is about to wake for a ready task, so the request is spent. Called before the
    /// wake, not after: a task made ready inside it must be able to ask again.
    pub(crate) fn take_wake_now(&self) {
        self.now_requested.store(false, Ordering::Relaxed);
    }

    fn lock(&self) -> MutexGuard<'_, Option<Instant>> {
        self.timer_wakeup
            .lock()
            .expect("the wakeup is never poisoned: nothing panics under its lock")
    }
}

impl Dispatcher for Wake {
    fn wake_at(&self, timer_wakeup: Option<Instant>) {
        // Set whole: the new time may be later than the old one, and `None` means no timer is
        // waiting.
        *self.lock() = timer_wakeup;
    }

    fn wake_now(&self) {
        // One notice at a time. The loop clears the flag as it takes the wake, so asking again
        // while one is pending adds nothing and asking again from inside that wake sends a
        // fresh one.
        if !self.now_requested.swap(true, Ordering::Relaxed) {
            let _ = self.notices.unbounded_send(Notice::WakeNow);
        }
    }

    /// On the program's executor, the next time the loop waits: a program has one thread until
    /// it has workers.
    fn dispatch(&self, work: Box<dyn FnOnce() + Send>) {
        wasdk::spawn(async move { work() });
    }
}

/// The framework's timer wakeup as one wait on the program's monotonic clock.
///
/// `wakeup` is the time the wait it holds was made for, so a wakeup that has not moved keeps its
/// wait and does not drift. It is forgotten the moment the wait is spent, which is what lets the
/// framework set the very same time again and get a new wait rather than a finished one polled
/// twice.
#[derive(Default)]
pub(crate) struct Alarm {
    wakeup: Option<Instant>,
    waiting: Option<Pin<Box<dyn Future<Output = ()>>>>,
}

impl Alarm {
    /// Waits until `wakeup`, or not at all when there is none.
    pub(crate) fn set(&mut self, wakeup: Option<Instant>) {
        if wakeup == self.wakeup {
            return;
        }
        self.wakeup = wakeup;
        self.waiting = wakeup.map(|at| {
            let wait = at.saturating_duration_since(Instant::now());
            let nanoseconds = u64::try_from(wait.as_nanos()).unwrap_or(u64::MAX);
            Box::pin(monotonic_clock::wait_for(nanoseconds)) as Pin<Box<_>>
        });
    }

    /// Resolves when the wakeup arrives, and never while none is set. Dropped before it
    /// resolves, which is every turn another event wins, the wait keeps its place.
    pub(crate) async fn ring(&mut self) {
        let Some(waiting) = self.waiting.as_mut() else {
            return future::pending().await;
        };
        waiting.await;
        self.wakeup = None;
        self.waiting = None;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn wake() -> (Wake, mpsc::UnboundedReceiver<Notice>) {
        let (sender, notices) = mpsc::unbounded();
        (Wake::new(sender), notices)
    }

    #[test]
    fn a_timer_wakeup_is_set_whole_and_may_move_later_or_away() {
        let (wake, _notices) = wake();
        let early = Instant::now();
        let late = early + Duration::from_millis(50);
        wake.wake_at(Some(late));
        assert_eq!(wake.timer_wakeup(), Some(late));
        wake.wake_at(Some(early));
        assert_eq!(wake.timer_wakeup(), Some(early));
        wake.wake_at(Some(late));
        assert_eq!(wake.timer_wakeup(), Some(late));
        wake.wake_at(None);
        assert_eq!(wake.timer_wakeup(), None);
    }

    #[test]
    fn a_wakeup_that_has_not_arrived_is_left_waiting() {
        let (wake, _notices) = wake();
        let later = Instant::now() + Duration::from_secs(1);
        wake.wake_at(Some(later));
        wake.timer_arrived(Instant::now());
        assert_eq!(wake.timer_wakeup(), Some(later));
        wake.timer_arrived(later);
        assert_eq!(wake.timer_wakeup(), None);
    }

    #[test]
    fn a_ready_task_notices_the_loop_once_until_the_loop_takes_it() {
        let (wake, mut notices) = wake();
        wake.wake_now();
        wake.wake_now();
        assert!(matches!(notices.try_recv(), Ok(Notice::WakeNow)));
        assert!(notices.try_recv().is_err(), "the second ask added nothing");
        wake.take_wake_now();
        wake.wake_now();
        assert!(matches!(notices.try_recv(), Ok(Notice::WakeNow)));
    }
}
