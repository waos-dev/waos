//! What prevents an app's freeze: each part of the engine that knows the app must keep running
//! takes a [`FreezeInhibitor`] for as long as that is true, and lets go of it after. The name is
//! the systems' own for this, as systemd-logind's inhibitor locks and Wayland's idle inhibitor.
//!
//! The lifecycle keeps them ([`FreezeInhibitors`]) and reads them in its one rule
//! (`execution_state`). The part that takes one knows nothing of freezing beyond this: when a
//! sound is audible is the audio's to know, and what a freeze is, is not.
//!
//! The invariant: an inhibitor counts from the moment it is taken until it is dropped, and every
//! one taken is dropped, since letting go is its `Drop`.

use std::time::Instant;

use super::Lifecycle;

/// What an app is doing that prevents its freeze.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FreezeInhibitorKind {
    /// It plays sound that is audible.
    AudioPlayback,
    /// It records sound.
    AudioCapture,
    /// It runs a camera.
    CameraCapture,
    /// It has a request yet to answer: to end, to close a view, for clipboard data it promised,
    /// an event on its menus or tray icons, files or links to open, a call of a service
    /// extension.
    PendingRequest,
}

/// Prevents an app's freeze until dropped.
pub(crate) struct FreezeInhibitor {
    kind: FreezeInhibitorKind,
    lifecycle: Lifecycle,
}

impl FreezeInhibitor {
    /// An inhibitor the lifecycle has counted already.
    pub(super) fn counted(kind: FreezeInhibitorKind, lifecycle: Lifecycle) -> FreezeInhibitor {
        FreezeInhibitor { kind, lifecycle }
    }
}

impl Drop for FreezeInhibitor {
    fn drop(&mut self) {
        self.lifecycle.freeze_inhibitor_dropped(self.kind);
    }
}

/// The inhibitors an app has now, and when the last of them went.
#[derive(Default)]
pub(super) struct FreezeInhibitors {
    taken: Vec<FreezeInhibitorKind>,
    /// When the app last had one and then none: what a freeze's delay runs from, so an app that
    /// falls silent between two sounds is not frozen in the gap.
    last_dropped: Option<Instant>,
}

impl FreezeInhibitors {
    pub(super) fn add(&mut self, kind: FreezeInhibitorKind) {
        self.taken.push(kind);
    }

    /// One of `kind` was dropped, at `now`.
    pub(super) fn remove(&mut self, kind: FreezeInhibitorKind, now: Instant) {
        let taken = self.taken.iter().position(|taken| *taken == kind);
        self.taken
            .swap_remove(taken.expect("an inhibitor dropped was taken"));
        if self.taken.is_empty() {
            self.last_dropped = Some(now);
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.taken.is_empty()
    }

    /// When the last inhibitor went, for an app that has none now and had one.
    pub(super) fn last_dropped(&self) -> Option<Instant> {
        self.last_dropped
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn an_app_is_inhibited_until_every_one_it_took_is_dropped() {
        let mut inhibitors = FreezeInhibitors::default();
        let now = Instant::now();
        assert!(inhibitors.is_empty());
        inhibitors.add(FreezeInhibitorKind::AudioPlayback);
        inhibitors.add(FreezeInhibitorKind::AudioPlayback);
        inhibitors.add(FreezeInhibitorKind::PendingRequest);
        inhibitors.remove(FreezeInhibitorKind::AudioPlayback, now);
        inhibitors.remove(FreezeInhibitorKind::PendingRequest, now);
        assert!(!inhibitors.is_empty());
        assert_eq!(inhibitors.last_dropped(), None, "it still has one");
        inhibitors.remove(FreezeInhibitorKind::AudioPlayback, now);
        assert!(inhibitors.is_empty());
    }

    #[test]
    fn when_the_last_one_went_is_kept() {
        let mut inhibitors = FreezeInhibitors::default();
        let now = Instant::now();
        inhibitors.add(FreezeInhibitorKind::CameraCapture);
        inhibitors.remove(FreezeInhibitorKind::CameraCapture, now);
        assert_eq!(inhibitors.last_dropped(), Some(now));
        let later = now + Duration::from_secs(1);
        inhibitors.add(FreezeInhibitorKind::AudioCapture);
        inhibitors.remove(FreezeInhibitorKind::AudioCapture, later);
        assert_eq!(inhibitors.last_dropped(), Some(later));
    }
}
