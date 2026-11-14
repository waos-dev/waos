//! Stopping a program where it stands, and letting it go on: the engine's freeze, which the
//! program is not told of.
//!
//! A frozen program stops inside its wasm at the next epoch tick and waits there until it is
//! thawed, then goes on from where it stopped. One waiting in a host call stops as soon as it
//! runs again. So a frozen program uses no CPU, whatever it was doing.
//!
//! Whether the program is frozen, and whether it stands at an epoch tick for its freeze, are told
//! as they change ([`Freezer::until_frozen`], [`Freezer::stopped_at_tick`]): a frozen program
//! waiting in a host call has not stopped there yet, and one between fuel yields is running.
//!
//! The invariant a later editor must keep: every stop a freeze made is let go by the thaw, and
//! nothing else lets one go, so a thawed program never waits and a frozen one never runs.

use std::future::Future;
use std::sync::Arc;

use futures::channel::oneshot;
use tokio::sync::watch;

/// A program's freeze: whether it is frozen, and where it stopped. A plain value, held under
/// the lock of the rule that decides it.
pub(super) struct Freezer {
    frozen: watch::Sender<bool>,
    /// One for each place the program stopped since it was frozen, which the thaw lets go on.
    stops: Vec<oneshot::Sender<()>>,
    /// Whether the program stands at an epoch tick for its freeze.
    at_tick: Arc<watch::Sender<bool>>,
}

impl Default for Freezer {
    fn default() -> Freezer {
        Freezer {
            frozen: watch::Sender::new(false),
            stops: Vec::new(),
            at_tick: Arc::new(watch::Sender::new(false)),
        }
    }
}

impl Freezer {
    /// Stops the program at its next epoch tick, until [`thaw`](Self::thaw). Answers whether
    /// this is a new freeze, rather than one that holds already.
    pub(super) fn freeze(&mut self) -> bool {
        !self.frozen.send_replace(true)
    }

    /// Lets the program go on from where it stopped.
    pub(super) fn thaw(&mut self) {
        self.frozen.send_replace(false);
        for stop in self.stops.drain(..) {
            // A stop whose program has gone has nobody to let go on.
            let _ = stop.send(());
        }
    }

    /// Whether the program is frozen, or will stop at its next epoch tick.
    pub(super) fn is_frozen(&self) -> bool {
        *self.frozen.borrow()
    }

    /// Resolves once the program is frozen, at once while it is, whether or not it has stopped
    /// yet.
    pub(super) fn until_frozen(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let mut frozen = self.frozen.subscribe();
        async move {
            // The sender is the freezer's, here as long as the program's lifecycle.
            let _ = frozen.wait_for(|frozen| *frozen).await;
        }
    }

    /// What the program's store does at an epoch tick: goes on (`None`), or, while frozen,
    /// stops there until the thaw, which is when the future answered resolves.
    pub(super) fn at_tick(&mut self) -> Option<impl Future<Output = ()> + Send + 'static + use<>> {
        let thawed = self.stop()?;
        tracing::debug!("the program stops for its freeze");
        self.at_tick.send_replace(true);
        let at_tick = Arc::clone(&self.at_tick);
        Some(async move {
            thawed.await;
            at_tick.send_replace(false);
            tracing::debug!("the program goes on, thawed");
        })
    }

    /// Resolves once the program stands at an epoch tick for its freeze, at once while it does.
    pub(super) fn stopped_at_tick(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let mut at_tick = self.at_tick.subscribe();
        async move {
            // The sender is the freezer's, here as long as the program's lifecycle.
            let _ = at_tick.wait_for(|at_tick| *at_tick).await;
        }
    }

    /// A stop while the program is frozen, let go by the thaw; `None` while it is not frozen.
    pub(super) fn stop(&mut self) -> Option<impl Future<Output = ()> + Send + 'static + use<>> {
        if !self.is_frozen() {
            return None;
        }
        let (stop, thawed) = oneshot::channel();
        self.stops.push(stop);
        Some(async move {
            let _ = thawed.await;
        })
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::*;

    #[test]
    fn a_program_goes_on_at_a_tick_until_it_is_frozen() {
        let mut freezer = Freezer::default();
        assert!(freezer.at_tick().is_none());
        assert!(!freezer.is_frozen());
    }

    #[test]
    fn a_frozen_program_waits_at_its_tick_until_the_thaw() {
        let mut freezer = Freezer::default();
        freezer.freeze();
        assert!(freezer.is_frozen());
        let mut stopped = freezer
            .at_tick()
            .expect("a frozen program stops at its tick")
            .boxed();
        assert_eq!((&mut stopped).now_or_never(), None, "it waits while frozen");
        freezer.thaw();
        assert_eq!(stopped.now_or_never(), Some(()), "the thaw lets it go on");
        assert!(freezer.at_tick().is_none());
    }

    #[test]
    fn a_program_is_told_frozen_from_the_freeze_until_the_thaw() {
        let mut freezer = Freezer::default();
        assert_eq!(freezer.until_frozen().now_or_never(), None);
        let frozen = freezer.until_frozen().boxed();
        freezer.freeze();
        assert_eq!(
            frozen.now_or_never(),
            Some(()),
            "the freeze tells who waits"
        );
        freezer.thaw();
        assert_eq!(freezer.until_frozen().now_or_never(), None);
    }

    #[test]
    fn a_program_stands_at_its_tick_from_its_stop_there_until_it_goes_on() {
        let mut freezer = Freezer::default();
        assert_eq!(freezer.stopped_at_tick().now_or_never(), None);
        freezer.freeze();
        assert_eq!(
            freezer.stopped_at_tick().now_or_never(),
            None,
            "frozen, it has not stopped yet"
        );
        let stopped = freezer.at_tick().expect("a frozen program stops").boxed();
        assert_eq!(freezer.stopped_at_tick().now_or_never(), Some(()));
        freezer.thaw();
        assert_eq!(stopped.now_or_never(), Some(()));
        assert_eq!(freezer.stopped_at_tick().now_or_never(), None, "it went on");
    }

    #[test]
    fn the_thaw_lets_every_stop_go_on() {
        let mut freezer = Freezer::default();
        freezer.freeze();
        let stops: Vec<_> = (0..2)
            .map(|_| {
                freezer
                    .at_tick()
                    .expect("a frozen program stops at every tick")
            })
            .collect();
        freezer.thaw();
        for stopped in stops {
            assert_eq!(stopped.now_or_never(), Some(()));
        }
    }
}
