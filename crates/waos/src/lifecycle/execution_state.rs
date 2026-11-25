//! What the engine lets an app do now, its execution state, and the one rule that decides it.
//! The name is the systems' own: Windows' `ApplicationExecutionState`, Apple's execution states.
//!
//! An app in sight runs. One out of sight is frozen: a hidden one once it has stayed hidden for
//! the policy's delay, so that switching back and forth costs nothing and an app told it is
//! hidden has time to save what it must not lose; a paused one at once, since the system itself
//! sent WaOS away. Whatever inhibits its freeze keeps it running meanwhile (`freeze_inhibitor`),
//! and the delay runs again from the moment the last inhibitor went. Why, and what was measured:
//! a frozen app uses no CPU, and gives back the driver's GPU memory about a second later.
//!
//! The lifecycle publishes the state, and every part of the engine that behaves differently for
//! an app that is not running reads it there, at its own door: the app's thread, which does not
//! enter a frozen app's code, and whoever submits its GPU work.
//!
//! The invariant: the rule is this function alone, of what it is handed, so it decides the same
//! whoever applies it and whenever.

use std::time::Duration;

use crate::bindings::wasdk::platform::lifecycle::LifecycleState;

/// What the engine lets an app do now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExecutionState {
    /// Its code runs.
    Running,
    /// None of its code runs, until it is unfrozen.
    Frozen,
}

/// When the engine freezes an app it keeps out of sight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreezePolicy {
    /// How long an app stays hidden before the engine freezes it on its own; none: only when
    /// WaOS is suspended, which is the default. A hidden app's window may be one the user can
    /// still reach and close, a card in Android's Recents, and an app frozen alone hears nothing
    /// of it until it is shown again.
    pub after_hidden: Option<Duration>,
}

/// What the rule weighs of an app, beside the policy.
#[derive(Clone, Copy, Debug)]
pub(super) struct FreezeFacts {
    pub(super) state: LifecycleState,
    /// How long the app has been in `state`.
    pub(super) in_state_for: Duration,
    /// How long it has been hidden or paused, one after the other counted as one stretch.
    pub(super) out_of_sight_for: Duration,
    /// Whether anything inhibits its freeze now.
    pub(super) inhibited: bool,
    /// How long ago its last inhibitor went, for an app that had one.
    pub(super) uninhibited_for: Option<Duration>,
}

/// The execution state of an app `facts` are true of, under `policy`.
pub(super) fn execution_state(facts: FreezeFacts, policy: &FreezePolicy) -> ExecutionState {
    if facts.inhibited {
        return ExecutionState::Running;
    }
    // A hidden app's delay runs through a suspension of WaOS; a paused one's from the suspension.
    let (delay, waited) = match facts.state {
        LifecycleState::Resumed | LifecycleState::Inactive => return ExecutionState::Running,
        LifecycleState::Hidden => match policy.after_hidden {
            Some(delay) => (delay, facts.out_of_sight_for),
            None => return ExecutionState::Running,
        },
        LifecycleState::Paused => (Duration::ZERO, facts.in_state_for),
    };
    // The delay runs from the later moment: that one, or when its last inhibitor went.
    let out_of_use_for = match facts.uninhibited_for {
        Some(uninhibited_for) => waited.min(uninhibited_for),
        None => waited,
    };
    if out_of_use_for >= delay {
        ExecutionState::Frozen
    } else {
        ExecutionState::Running
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ExecutionState::{Frozen, Running};
    use LifecycleState::{Hidden, Inactive, Paused, Resumed};

    const DELAY: Duration = Duration::from_secs(5);
    const A_MOMENT: Duration = Duration::from_millis(1);

    const AFTER_DELAY: FreezePolicy = FreezePolicy {
        after_hidden: Some(DELAY),
    };

    /// An app in `state` for `in_state_for`, in sight before, nothing inhibiting its freeze, nor
    /// ever having.
    fn app(state: LifecycleState, in_state_for: Duration) -> FreezeFacts {
        FreezeFacts {
            state,
            in_state_for,
            out_of_sight_for: in_state_for,
            inhibited: false,
            uninhibited_for: None,
        }
    }

    #[test]
    fn an_app_in_sight_runs_however_long() {
        for state in [Resumed, Inactive] {
            assert_eq!(
                execution_state(app(state, DELAY * 2), &AFTER_DELAY),
                Running
            );
        }
    }

    #[test]
    fn a_hidden_app_is_frozen_once_the_delay_is_over() {
        let before = app(Hidden, DELAY - A_MOMENT);
        assert_eq!(execution_state(before, &AFTER_DELAY), Running);
        assert_eq!(execution_state(app(Hidden, DELAY), &AFTER_DELAY), Frozen);
    }

    #[test]
    fn the_default_policy_freezes_an_app_only_when_waos_is_suspended() {
        let policy = FreezePolicy::default();
        assert_eq!(execution_state(app(Hidden, DELAY * 2), &policy), Running);
        assert_eq!(
            execution_state(app(Paused, Duration::ZERO), &policy),
            Frozen
        );
    }

    #[test]
    fn a_paused_app_is_frozen_at_once() {
        let paused = app(Paused, Duration::ZERO);
        assert_eq!(execution_state(paused, &AFTER_DELAY), Frozen);
    }

    #[test]
    fn an_inhibitor_keeps_an_app_running_hidden_or_paused() {
        for state in [Hidden, Paused] {
            let inhibited = FreezeFacts {
                inhibited: true,
                ..app(state, DELAY * 2)
            };
            assert_eq!(execution_state(inhibited, &AFTER_DELAY), Running);
        }
    }

    /// An app hidden long ago that fell silent a moment ago: the gap between two sounds.
    #[test]
    fn the_delay_runs_again_from_the_moment_the_last_inhibitor_went() {
        let silent_for = |uninhibited_for| FreezeFacts {
            uninhibited_for: Some(uninhibited_for),
            ..app(Hidden, DELAY * 10)
        };
        assert_eq!(execution_state(silent_for(A_MOMENT), &AFTER_DELAY), Running);
        assert_eq!(execution_state(silent_for(DELAY), &AFTER_DELAY), Frozen);
    }

    /// WaOS was suspended and resumed while the app was hidden: hidden again only a moment ago,
    /// and out of sight all along.
    #[test]
    fn a_hidden_apps_delay_runs_through_a_suspension() {
        let hidden_again = |out_of_sight_for| FreezeFacts {
            out_of_sight_for,
            ..app(Hidden, A_MOMENT)
        };
        let before = hidden_again(DELAY - A_MOMENT);
        assert_eq!(execution_state(before, &AFTER_DELAY), Running);
        assert_eq!(execution_state(hidden_again(DELAY), &AFTER_DELAY), Frozen);
    }

    /// An app that answered a request long ago and was hidden a moment ago.
    #[test]
    fn the_delay_runs_from_going_out_of_sight_where_that_is_later() {
        let hidden_a_moment_ago = FreezeFacts {
            uninhibited_for: Some(DELAY * 10),
            ..app(Hidden, A_MOMENT)
        };
        assert_eq!(execution_state(hidden_a_moment_ago, &AFTER_DELAY), Running);
    }

    #[test]
    fn a_paused_app_that_answered_what_it_owed_is_frozen_again_at_once() {
        let answered = FreezeFacts {
            uninhibited_for: Some(Duration::ZERO),
            ..app(Paused, DELAY)
        };
        assert_eq!(execution_state(answered, &AFTER_DELAY), Frozen);
    }
}
