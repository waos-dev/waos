//! The program's lifecycle state now, and the streams its changes go to.
//!
//! The state is what the program's views give it, except while the app is suspended, when it is
//! `paused` whatever they give; the views' state is kept meanwhile, for the resume. The
//! invariant: a stream hears a change only when the state is another than it was, so the state
//! may be derived, and set, as often as its views change.

use std::sync::Mutex;

use futures::channel::mpsc;

use crate::bindings::wasdk::platform::lifecycle::LifecycleState;
use crate::channel::Channel;
use crate::sync::locked;

/// The program's state now, and where each change of it goes.
pub(super) struct CurrentState {
    standing: Mutex<Standing>,
    changes: Channel<LifecycleState>,
}

/// Where the program stands: the state its views give it, and whether the app is suspended,
/// which puts it in `paused` over that.
#[derive(Clone, Copy)]
struct Standing {
    of_views: LifecycleState,
    app_suspended: bool,
}

impl Standing {
    fn state(self) -> LifecycleState {
        if self.app_suspended {
            LifecycleState::Paused
        } else {
            self.of_views
        }
    }
}

impl CurrentState {
    /// `Resumed`, the state a program starts in, until its views put it in another.
    pub(super) fn new() -> CurrentState {
        CurrentState {
            standing: Mutex::new(Standing {
                of_views: LifecycleState::Resumed,
                app_suspended: false,
            }),
            changes: Channel::default(),
        }
    }

    pub(super) fn now(&self) -> LifecycleState {
        locked(&self.standing).state()
    }

    /// Each change from now on, in the order they happened.
    pub(super) fn changes(&self) -> mpsc::UnboundedReceiver<LifecycleState> {
        self.changes.subscribe()
    }

    /// The program's views put it in `state`: it is in it, unless the app is suspended, and
    /// then once the app resumes.
    pub(super) fn of_views(&self, state: LifecycleState) {
        self.change(|standing| standing.of_views = state);
    }

    /// The app is suspended: the program is `paused`, whatever its views give.
    pub(super) fn app_suspending(&self) {
        self.change(|standing| standing.app_suspended = true);
    }

    /// The app resumed: the program is back in the state its views give.
    pub(super) fn app_resumed(&self) {
        self.change(|standing| standing.app_suspended = false);
    }

    /// Applies `change` and, where the state is another than it was, sends it to every stream.
    fn change(&self, change: impl FnOnce(&mut Standing)) {
        let mut standing = locked(&self.standing);
        let before = standing.state();
        change(&mut standing);
        let after = standing.state();
        if after == before {
            return;
        }
        // Sent under the lock, so two changes from two threads reach every stream in the order
        // the state took them.
        self.changes.send(after);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LifecycleState::{Hidden, Inactive, Paused, Resumed};

    #[test]
    fn a_program_starts_resumed() {
        assert_eq!(CurrentState::new().now(), Resumed);
    }

    #[test]
    fn a_change_reaches_every_stream_in_order_and_the_state_follows() {
        let state = CurrentState::new();
        let (mut first, mut second) = (state.changes(), state.changes());
        state.of_views(Inactive);
        state.of_views(Hidden);
        assert_eq!(state.now(), Hidden);
        for states in [&mut first, &mut second] {
            assert_eq!(states.try_recv().unwrap(), Inactive);
            assert_eq!(states.try_recv().unwrap(), Hidden);
        }
    }

    #[test]
    fn the_same_state_again_is_not_a_change() {
        let state = CurrentState::new();
        let mut states = state.changes();
        state.of_views(Resumed);
        state.of_views(Hidden);
        state.of_views(Hidden);
        assert_eq!(states.try_recv().unwrap(), Hidden);
        assert!(states.try_recv().is_err(), "heard once");
    }

    #[test]
    fn a_stream_hears_only_the_changes_after_it_opened() {
        let state = CurrentState::new();
        state.of_views(Inactive);
        let mut states = state.changes();
        assert!(states.try_recv().is_err(), "an earlier change is not heard");
        assert_eq!(state.now(), Inactive);
        state.of_views(Resumed);
        assert_eq!(states.try_recv().unwrap(), Resumed);
    }

    #[test]
    fn a_stream_that_reads_late_hears_every_change_and_the_latest_last() {
        let state = CurrentState::new();
        let mut states = state.changes();
        for _ in 0..8 {
            state.of_views(Hidden);
            state.of_views(Resumed);
        }
        let heard: Vec<_> = std::iter::from_fn(|| states.try_recv().ok()).collect();
        assert_eq!(heard.len(), 16);
        assert_eq!(heard.last(), Some(&Resumed));
    }

    #[test]
    fn the_app_suspending_pauses_the_program_and_resuming_puts_it_back() {
        let state = CurrentState::new();
        let mut states = state.changes();
        state.of_views(Inactive);
        state.app_suspending();
        assert_eq!(state.now(), Paused);
        state.app_resumed();
        assert_eq!(state.now(), Inactive, "back to what its views give");
        let heard: Vec<_> = std::iter::from_fn(|| states.try_recv().ok()).collect();
        assert_eq!(heard, [Inactive, Paused, Inactive]);
    }

    #[test]
    fn the_views_are_followed_while_the_app_is_suspended_and_heard_at_the_resume() {
        let state = CurrentState::new();
        let mut states = state.changes();
        state.app_suspending();
        state.of_views(Hidden);
        assert_eq!(state.now(), Paused, "paused, whatever the views give");
        state.app_resumed();
        assert_eq!(state.now(), Hidden);
        let heard: Vec<_> = std::iter::from_fn(|| states.try_recv().ok()).collect();
        assert_eq!(heard, [Paused, Hidden]);
    }

    #[test]
    fn suspending_twice_is_heard_once() {
        let state = CurrentState::new();
        let mut states = state.changes();
        state.app_suspending();
        state.app_suspending();
        assert_eq!(states.try_recv().unwrap(), Paused);
        assert!(states.try_recv().is_err());
    }
}
