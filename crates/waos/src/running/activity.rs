//! Whether a program is running its code at this moment, in any of its instances, and waiting
//! until it is not.
//!
//! Each instance's thread polls the one future that is its run. The instance runs its code inside
//! that poll, on wasmtime's fiber, and the poll returns pending when it waits: in a host call, or
//! stopped by its freeze. So running is being inside the poll, which the poll itself reports from
//! here, and the program is idle when no instance is.
//!
//! The invariant: `idle` resolves only while every instance's thread is outside its poll, so at
//! that moment nothing of the program executes. It says nothing about the next moment: what keeps
//! a program from running again is its freeze, not this.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::sync::watch;

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

    /// `run`, one instance's, reporting into this: the instance is running while `run` is polled.
    pub(super) fn watch<F: Future + Send>(self: &Arc<Self>, run: F) -> Watched<F> {
        Watched {
            run: Box::pin(run),
            activity: Arc::clone(self),
            polling: false,
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

/// One instance's run, reporting whether it is being polled.
pub(super) struct Watched<F> {
    run: Pin<Box<F>>,
    activity: Arc<Activity>,
    /// Whether the run is inside its poll now, as `Drop` needs to know.
    polling: bool,
}

impl<F: Future> Future for Watched<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        self.activity.entered();
        self.polling = true;
        let polled = self.run.as_mut().poll(cx);
        self.polling = false;
        self.activity.left();
        polled
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
    use std::task::Waker;

    use futures::FutureExt;
    use futures::channel::oneshot;

    use super::*;

    #[test]
    fn a_program_is_idle_before_it_runs_and_once_it_has_returned() {
        let activity = Arc::new(Activity::new());
        assert_eq!(activity.idle().now_or_never(), Some(()));
        let mut run = activity.watch(async { 7 });
        assert_eq!((&mut run).now_or_never(), Some(7));
        assert_eq!(activity.idle().now_or_never(), Some(()));
    }

    #[test]
    fn a_program_waiting_in_a_host_call_is_idle_and_one_in_its_code_is_not() {
        let activity = Arc::new(Activity::new());
        let (host_call, waiting) = oneshot::channel::<()>();
        let watching = Arc::clone(&activity);
        let mut run = activity.watch(async move {
            // Inside the poll, the program runs.
            assert_eq!(watching.idle().now_or_never(), None);
            let _ = waiting.await;
        });
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
        let mut waiting_instance = activity.watch(async move {
            let _ = waiting.await;
        });
        let mut context = Context::from_waker(Waker::noop());
        assert!(
            Pin::new(&mut waiting_instance)
                .poll(&mut context)
                .is_pending()
        );
        let mut running_instance = activity.watch(async move {
            assert_eq!(
                watching.idle().now_or_never(),
                None,
                "one instance in its code keeps the program running"
            );
        });
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
        let run = activity.watch(std::future::pending::<()>());
        let mut context = Context::from_waker(Waker::noop());
        let mut run = run;
        assert!(Pin::new(&mut run).poll(&mut context).is_pending());
        let mut idle = activity.idle().boxed();
        assert_eq!((&mut idle).now_or_never(), Some(()), "pending is idle");
        drop(run);
        assert_eq!(activity.idle().now_or_never(), Some(()));
    }
}
