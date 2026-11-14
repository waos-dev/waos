//! A program's GPU work not yet finished, as the engine counts it: each frame it presented,
//! waited for by its view's finisher thread, and each wait for a device's whole queue once the
//! program stands still. What the app's suspending waits for, so that nothing is on the GPU when
//! the platform suspends the app: iOS ends an app whose GPU work is still running then.
//!
//! The invariant: a piece of work is counted from the moment it is handed to a thread that
//! waits for it until that wait is over, so `none` resolves only once the GPU has finished what
//! was counted; what nobody waits for is not counted.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::watch;

/// How much of a program's GPU work is in flight.
pub(crate) struct InFlight {
    count: watch::Sender<usize>,
}

/// One piece of work in flight, counted until this is dropped.
pub(crate) struct Finishing(Arc<InFlight>);

impl InFlight {
    pub(crate) fn new() -> Arc<InFlight> {
        Arc::new(InFlight {
            count: watch::Sender::new(0),
        })
    }

    /// One piece of work more, in flight until what is answered is dropped.
    pub(crate) fn begin(self: &Arc<Self>) -> Finishing {
        self.count.send_modify(|count| *count += 1);
        Finishing(Arc::clone(self))
    }

    /// Resolves once nothing counted is in flight.
    pub(crate) fn none(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let mut count = self.count.subscribe();
        async move {
            // A sender gone is a program gone, with nothing in flight.
            let _ = count.wait_for(|count| *count == 0).await;
        }
    }
}

impl Drop for Finishing {
    fn drop(&mut self) {
        self.0.count.send_modify(|count| *count -= 1);
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::*;

    #[test]
    fn nothing_in_flight_resolves_at_once() {
        let in_flight = InFlight::new();
        assert_eq!(in_flight.none().now_or_never(), Some(()));
    }

    #[test]
    fn none_resolves_once_every_piece_of_work_is_finished() {
        let in_flight = InFlight::new();
        let first = in_flight.begin();
        let second = in_flight.begin();
        let mut none = in_flight.none().boxed();
        assert_eq!((&mut none).now_or_never(), None);
        drop(first);
        assert_eq!((&mut none).now_or_never(), None, "one is still in flight");
        drop(second);
        assert_eq!(none.now_or_never(), Some(()));
    }
}
