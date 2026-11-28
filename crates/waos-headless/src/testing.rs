//! What WaOS's tests wait and hold with, the engine's and the app's alike: one deadline for every
//! wait ([`WITHIN`]), a wait for a future ([`within`]), a wait that wakes as what it watches
//! changes ([`Changes`]), and a hold a stand-in waits on until the test lets it go ([`Hold`]);
//! and, for what nothing announces yet, a wait that looks again and again ([`poll_until`]). The
//! rest of the kit lives beside what it stands in for, and `docs/tools/tests.md` lists it.
//!
//! The invariant a later editor will break: no wait here but [`poll_until`] sleeps. Each wakes as
//! the thing it waits on changes, and fails once [`WITHIN`] has passed, which only a broken build
//! reaches; a part a test waits on announces each change through its [`Changes`], or the wait
//! never sees it.

use std::future::Future;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::watch;

/// How long a test waits for what it expects before it fails: longer than any passing test takes
/// alone, so a hung wait costs seconds, not minutes.
pub const WITHIN: Duration = Duration::from_secs(15);

/// What `future` resolves with, failing after [`WITHIN`] with the line that waited. It blocks the
/// test's thread and runs `future` on a thread of its own.
#[track_caller]
pub fn within<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> T {
    let waiting_at = std::panic::Location::caller();
    let (resolved, came) = mpsc::channel();
    thread::spawn(move || {
        // A test that timed out has dropped the receiver.
        let _ = resolved.send(futures::executor::block_on(future));
    });
    came.recv_timeout(WITHIN).unwrap_or_else(|_| {
        panic!("what the test waited for at {waiting_at} never came within {WITHIN:?}")
    })
}

/// The changes of something a test reads, which its keeper announces ([`Changes::announce`]) and
/// a test waits on ([`Changes::wait_until`], or [`Changes::until`] in an async test): a
/// stand-in's records, a shell's windows, a program's output. One `Changes` may serve many things: a wait checks what it waits for again
/// at each change, so a change to something else only costs a check.
pub struct Changes {
    /// How many changes were announced.
    count: Mutex<u64>,
    changed: Condvar,
    /// The same changes, for an async wait, which must not block its runtime's thread.
    announced: watch::Sender<()>,
}

impl Default for Changes {
    fn default() -> Changes {
        Changes {
            count: Mutex::default(),
            changed: Condvar::new(),
            announced: watch::Sender::new(()),
        }
    }
}

impl Changes {
    /// Says that what this watches changed, waking every wait.
    pub fn announce(&self) {
        *self.count() += 1;
        self.changed.notify_all();
        self.announced.send_replace(());
    }

    /// Resolves once `done`, checked now and after each change, failing after [`WITHIN`] with
    /// `what` was awaited: [`wait_until`](Self::wait_until) for an async test, on tokio's clock.
    pub async fn until(&self, what: &str, mut done: impl FnMut() -> bool) {
        let mut announced = self.announced.subscribe();
        let waited = tokio::time::timeout(WITHIN, async {
            loop {
                announced.borrow_and_update();
                if done() {
                    return;
                }
                // The sender lives as long as `self`, which this borrows.
                let _ = announced.changed().await;
            }
        });
        waited
            .await
            .unwrap_or_else(|_| panic!("never within {WITHIN:?}: {what}"));
    }

    /// Returns once `done`, checked now and after each change, failing after [`WITHIN`] with
    /// `what` was awaited. `done` reads what changed under its keeper's own lock.
    #[track_caller]
    pub fn wait_until(&self, what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + WITHIN;
        loop {
            let seen = *self.count();
            if done() {
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "never within {WITHIN:?}: {what}");
            let (_count, waited) = self
                .changed
                .wait_timeout_while(self.count(), left, |count| *count == seen)
                .unwrap_or_else(PoisonError::into_inner);
            assert!(!waited.timed_out(), "never within {WITHIN:?}: {what}");
        }
    }

    fn count(&self) -> MutexGuard<'_, u64> {
        self.count.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Returns once `done`, looking again every few milliseconds, failing after [`WITHIN`] with `what`
/// was awaited: only for state that nothing announces, such as a file a program writes, where a
/// [`Changes`] cannot follow it. Each use says beside it what would let it wait on a change.
#[track_caller]
pub fn poll_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WITHIN;
    while !done() {
        assert!(Instant::now() < deadline, "never within {WITHIN:?}: {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A hold a test takes on a stand-in for something slow outside the code under test, a compile or
/// a download: while the test holds it, the stand-in waits ([`Hold::wait_for_release`]), so the
/// test sees the state that lasts while the slow thing runs instead of guessing with a timer how
/// long it lasts.
#[derive(Default)]
pub struct Hold {
    /// How many guards are out.
    guards: Mutex<usize>,
    released: Condvar,
}

/// A test's hold on a [`Hold`]: the stand-in waits until every guard is dropped.
pub struct HoldGuard(Arc<Hold>);

impl Hold {
    /// Takes the hold: what the stand-in begins from now waits until the guard is dropped.
    pub fn take(self: &Arc<Self>) -> HoldGuard {
        *self.guards() += 1;
        HoldGuard(Arc::clone(self))
    }

    /// Returns once no guard is out: what the stand-in calls before it does its work.
    pub fn wait_for_release(&self) {
        let _released = self
            .released
            .wait_while(self.guards(), |guards| *guards > 0)
            .unwrap_or_else(PoisonError::into_inner);
    }

    fn guards(&self) -> MutexGuard<'_, usize> {
        self.guards.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for HoldGuard {
    fn drop(&mut self) {
        *self.0.guards() -= 1;
        self.0.released.notify_all();
    }
}
