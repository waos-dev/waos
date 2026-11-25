//! Where a program's code is entered, which is where its freeze takes: whether it is running its
//! code at this moment, in any of its instances, and waiting until it is not.
//!
//! Each instance's thread polls the one future that is its run. The instance runs its code inside
//! that poll, on wasmtime's fiber, and what it waits for through the engine is polled there too:
//! a timer, a read. The poll returns pending when it waits. So running is being inside the poll,
//! which the poll itself reports from here, and the program is idle when no instance is.
//!
//! A frozen program's run is not polled: its thread reads the program's execution state as it
//! comes to poll, and goes no further while that is `Frozen`. Neither its code nor what it waits
//! for is looked at until it is unfrozen, and what came due meanwhile is found then. Code busy
//! inside the poll steps out at its next epoch tick (`store`), and does not come back in.
//!
//! The invariant: `idle` resolves only while every instance's thread is outside its run, so at
//! that moment nothing of the program executes; and a thread counts as inside before it reads the
//! state, so once the program is frozen and idle, nothing of it runs until it is unfrozen.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::sync::watch;

use crate::lifecycle::{ExecutionState, Lifecycle};

/// How many of the program's instances are in their code now, as their threads' polls report it.
pub(super) struct Activity {
    running: watch::Sender<usize>,
}

impl Activity {
    /// A program not running yet.
    pub(super) fn new() -> Activity {
        Activity {
            running: watch::Sender::new(0),
        }
    }

    /// `run`, one instance's of the program whose lifecycle is `lifecycle`, reporting into this:
    /// the instance is running while `run` is polled, which it is not while the program is
    /// frozen.
    pub(super) fn watch<F: Future + Send>(
        self: &Arc<Self>,
        run: F,
        lifecycle: &Lifecycle,
    ) -> Watched<F> {
        Watched {
            run: Box::pin(run),
            activity: Arc::clone(self),
            polling: false,
            execution_state: lifecycle.execution_state(),
            unfrozen: None,
        }
    }

    /// Resolves once no instance of the program is running: each waiting, frozen or ended.
    pub(super) fn idle(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let mut running = self.running.subscribe();
        async move {
            // A sender gone is a program gone: it ended.
            let _ = running.wait_for(|running| *running == 0).await;
        }
    }

    fn entered(&self) {
        self.running.send_modify(|running| *running += 1);
    }

    fn left(&self) {
        self.running.send_modify(|running| *running -= 1);
    }
}

/// One instance's run, polled only while its program is not frozen, and reporting whether it is
/// being polled.
pub(super) struct Watched<F> {
    run: Pin<Box<F>>,
    activity: Arc<Activity>,
    /// Whether the run is inside its poll now, as `Drop` needs to know.
    polling: bool,
    execution_state: watch::Receiver<ExecutionState>,
    /// What wakes the instance's thread once the program is unfrozen, while it is frozen.
    unfrozen: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

impl<F: Future> Future for Watched<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        self.activity.entered();
        self.polling = true;
        let polled = if self.is_frozen(cx) {
            Poll::Pending
        } else {
            self.run.as_mut().poll(cx)
        };
        self.polling = false;
        self.activity.left();
        polled
    }
}

impl<F> Watched<F> {
    /// Whether the program is frozen, the task woken once it no longer is.
    fn is_frozen(&mut self, cx: &mut Context<'_>) -> bool {
        if *self.execution_state.borrow() != ExecutionState::Frozen {
            self.unfrozen = None;
            return false;
        }
        let mut execution_state = self.execution_state.clone();
        let unfrozen = self.unfrozen.get_or_insert_with(|| {
            Box::pin(async move {
                // A lifecycle gone is a program gone: nothing is left to run.
                let running_again = |state: &ExecutionState| *state != ExecutionState::Frozen;
                let _ = execution_state.wait_for(running_again).await;
            })
        });
        if unfrozen.as_mut().poll(cx).is_pending() {
            return true;
        }
        self.unfrozen = None;
        false
    }
}

impl<F> Drop for Watched<F> {
    /// A run dropped mid-way, by `stop`, runs no more; one dropped from inside its poll, as a
    /// panic unwinding through it is, leaves.
    fn drop(&mut self) {
        if self.polling {
            self.activity.left();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Wake, Waker};

    use futures::FutureExt;
    use futures::channel::oneshot;

    use super::*;

    #[test]
    fn a_program_is_idle_before_it_runs_and_once_it_has_returned() {
        let activity = Arc::new(Activity::new());
        assert_eq!(activity.idle().now_or_never(), Some(()));
        let mut run = activity.watch(async { 7 }, &Lifecycle::default());
        assert_eq!((&mut run).now_or_never(), Some(7));
        assert_eq!(activity.idle().now_or_never(), Some(()));
    }

    #[test]
    fn a_program_waiting_in_a_host_call_is_idle_and_one_in_its_code_is_not() {
        let activity = Arc::new(Activity::new());
        let (host_call, waiting) = oneshot::channel::<()>();
        let watching = Arc::clone(&activity);
        let in_its_code_then_waiting = async move {
            // Inside the poll, the program runs.
            assert_eq!(watching.idle().now_or_never(), None);
            let _ = waiting.await;
        };
        let mut run = activity.watch(in_its_code_then_waiting, &Lifecycle::default());
        let mut context = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut run).poll(&mut context).is_pending());
        assert_eq!(
            activity.idle().now_or_never(),
            Some(()),
            "waiting in the host call, it is idle"
        );
        host_call.send(()).unwrap();
        assert!(Pin::new(&mut run).poll(&mut context).is_ready());
    }

    #[test]
    fn a_program_is_running_while_any_instance_is() {
        let activity = Arc::new(Activity::new());
        let (host_call, waiting) = oneshot::channel::<()>();
        let watching = Arc::clone(&activity);
        let lifecycle = Lifecycle::default();
        let waits = async move {
            let _ = waiting.await;
        };
        let mut waiting_instance = activity.watch(waits, &lifecycle);
        let mut context = Context::from_waker(Waker::noop());
        assert!(
            Pin::new(&mut waiting_instance)
                .poll(&mut context)
                .is_pending()
        );
        let runs = async move {
            assert_eq!(
                watching.idle().now_or_never(),
                None,
                "one instance in its code keeps the program running"
            );
        };
        let mut running_instance = activity.watch(runs, &lifecycle);
        assert!(
            Pin::new(&mut running_instance)
                .poll(&mut context)
                .is_ready()
        );
        assert_eq!(activity.idle().now_or_never(), Some(()));
        host_call.send(()).unwrap();
        assert!(
            Pin::new(&mut waiting_instance)
                .poll(&mut context)
                .is_ready()
        );
    }

    #[test]
    fn a_run_dropped_leaves_the_program_idle_and_wakes_who_waits() {
        let activity = Arc::new(Activity::new());
        let run = activity.watch(std::future::pending::<()>(), &Lifecycle::default());
        let mut context = Context::from_waker(Waker::noop());
        let mut run = run;
        assert!(Pin::new(&mut run).poll(&mut context).is_pending());
        let mut idle = activity.idle().boxed();
        assert_eq!((&mut idle).now_or_never(), Some(()), "pending is idle");
        drop(run);
        assert_eq!(activity.idle().now_or_never(), Some(()));
    }

    /// A waker that counts how often it woke its task.
    struct CountedWakes(AtomicUsize);

    impl Wake for CountedWakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_frozen_programs_run_is_not_polled_until_it_is_unfrozen() {
        let activity = Arc::new(Activity::new());
        let lifecycle = Lifecycle::default();
        let polls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&polls);
        let counts_its_polls = future::poll_fn(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Poll::<()>::Pending
        });
        let mut run = activity.watch(counts_its_polls, &lifecycle);
        let wakes = Arc::new(CountedWakes(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wakes));
        let mut context = Context::from_waker(&waker);

        assert!(Pin::new(&mut run).poll(&mut context).is_pending());
        assert_eq!(polls.load(Ordering::SeqCst), 1, "running, it is polled");

        lifecycle.app_suspending();
        assert!(Pin::new(&mut run).poll(&mut context).is_pending());
        assert!(Pin::new(&mut run).poll(&mut context).is_pending());
        assert_eq!(polls.load(Ordering::SeqCst), 1, "frozen, it is not");
        assert_eq!(activity.idle().now_or_never(), Some(()));
        assert_eq!(wakes.0.load(Ordering::SeqCst), 0, "nothing wakes it yet");

        lifecycle.app_resumed();
        assert_eq!(
            wakes.0.load(Ordering::SeqCst),
            1,
            "unfrozen, its thread is woken"
        );
        assert!(Pin::new(&mut run).poll(&mut context).is_pending());
        assert_eq!(polls.load(Ordering::SeqCst), 2, "and polls it again");
    }
}
