//! Where GPUI's executors put their work: one thread's queues, which the event loop runs between
//! the platform's events.
//!
//! A program has one thread, so GPUI's foreground and background work interleave on it. Three
//! queues keep the order GPUI needs: timers by their deadline, the main thread's runnables, then
//! the background's by priority. A foreground timer is not held behind queued background work,
//! though a runnable that runs cannot be interrupted, so a long background step delays everything
//! behind it (the 10-16 research measured it).
//!
//! The invariant: GPUI reaches the dispatcher from inside wakers, which must be `Send`, so the
//! queues sit behind a mutex though one thread uses them; a runnable is always run with the lock
//! released, since running it may dispatch more.

use std::cmp::{Ordering as CmpOrdering, Reverse};
use std::collections::{BinaryHeap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use gpui::{PlatformDispatcher, Priority, RunnableVariant};

use crate::events::Notice;

/// GPUI's dispatcher on the program's one thread.
pub(crate) struct WasdkDispatcher {
    queues: Mutex<Queues>,
    /// Whether a wake is on its way to the loop already.
    wake_sent: AtomicBool,
    notices: mpsc::UnboundedSender<Notice>,
}

#[derive(Default)]
struct Queues {
    main: VecDeque<RunnableVariant>,
    background: BinaryHeap<QueuedRunnable>,
    timers: BinaryHeap<Reverse<Timer>>,
    sequence: u64,
}

/// A background runnable, ordered by its priority, then by when it was queued.
struct QueuedRunnable {
    rank: u8,
    sequence: u64,
    runnable: RunnableVariant,
}

struct Timer {
    deadline: Instant,
    sequence: u64,
    runnable: RunnableVariant,
}

impl WasdkDispatcher {
    pub(crate) fn new(notices: mpsc::UnboundedSender<Notice>) -> WasdkDispatcher {
        WasdkDispatcher {
            queues: Mutex::new(Queues::default()),
            wake_sent: AtomicBool::new(false),
            notices,
        }
    }

    /// The next piece of work that is due: a timer that has come, else the main thread's, else
    /// the background's.
    pub(crate) fn next_work(&self, now: Instant) -> Option<RunnableVariant> {
        let mut queues = self.lock();
        if queues
            .timers
            .peek()
            .is_some_and(|Reverse(timer)| timer.deadline <= now)
        {
            return queues.timers.pop().map(|Reverse(timer)| timer.runnable);
        }
        if let Some(runnable) = queues.main.pop_front() {
            return Some(runnable);
        }
        queues.background.pop().map(|queued| queued.runnable)
    }

    /// The deadline of the earliest timer, for the loop's alarm.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.lock()
            .timers
            .peek()
            .map(|Reverse(timer)| timer.deadline)
    }

    /// Whether work other than a timer waits.
    pub(crate) fn has_ready_work(&self) -> bool {
        let queues = self.lock();
        !queues.main.is_empty() || !queues.background.is_empty()
    }

    /// The loop takes the wake: a dispatch after this sends another.
    pub(crate) fn take_wake(&self) {
        self.wake_sent.store(false, Ordering::Relaxed);
    }

    fn push(&self, place: impl FnOnce(&mut Queues, u64)) {
        {
            let mut queues = self.lock();
            let sequence = queues.sequence;
            queues.sequence += 1;
            place(&mut queues, sequence);
        }
        self.wake();
    }

    fn wake(&self) {
        if !self.wake_sent.swap(true, Ordering::Relaxed) {
            let _ = self.notices.unbounded_send(Notice::Wake);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queues> {
        self.queues
            .lock()
            .expect("nothing panics while the queues are locked")
    }
}

impl PlatformDispatcher for WasdkDispatcher {
    fn is_main_thread(&self) -> bool {
        true
    }

    fn dispatch(&self, runnable: RunnableVariant, priority: Priority) {
        self.push(|queues, sequence| {
            queues.background.push(QueuedRunnable {
                rank: rank(priority),
                sequence,
                runnable,
            })
        });
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, _priority: Priority) {
        self.push(|queues, _| queues.main.push_back(runnable));
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        let deadline = Instant::now() + duration;
        self.push(|queues, sequence| {
            queues.timers.push(Reverse(Timer {
                deadline,
                sequence,
                runnable,
            }))
        });
    }

    fn spawn_realtime(&self, function: Box<dyn FnOnce() + Send>) {
        // No thread to give it: it runs as the next piece of main-thread work.
        let _ = self.notices.unbounded_send(Notice::Realtime(function));
    }
}

/// Higher runs first.
fn rank(priority: Priority) -> u8 {
    match priority {
        Priority::RealtimeAudio => 4,
        Priority::High => 3,
        Priority::Medium => 2,
        Priority::Low => 1,
    }
}

impl PartialEq for QueuedRunnable {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == CmpOrdering::Equal
    }
}
impl Eq for QueuedRunnable {}
impl PartialOrd for QueuedRunnable {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueuedRunnable {
    /// The higher rank first, then the one queued first.
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.rank
            .cmp(&other.rank)
            .then(other.sequence.cmp(&self.sequence))
    }
}

impl PartialEq for Timer {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == CmpOrdering::Equal
    }
}
impl Eq for Timer {}
impl PartialOrd for Timer {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl Ord for Timer {
    /// The earlier deadline is the lesser, then the one set first.
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.deadline
            .cmp(&other.deadline)
            .then(self.sequence.cmp(&other.sequence))
    }
}
