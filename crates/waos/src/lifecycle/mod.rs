//! `wasdk:platform/lifecycle`: how much of a program its user can see and reach, the platform
//! asking whether it may end, what the user opened with it, and the freeze that follows from them.
//!
//! A program's [`Lifecycle`] is held by its views, which derive its state from how they stand on
//! screen; by the engine, which puts every program in `paused` while the app is suspended, asks
//! through it before the platform quits, and keeps its freeze by the engine's clock, at each of
//! its ticks; and by the program's store, which stops at an epoch tick while it is frozen. The
//! program reads its state and the requests from its side. When the program was hidden or shown
//! is read from the engine's clock too (`engine_time`), never the system's, so a test that moves
//! the engine's time moves the freeze's delay with it.
//!
//! What the engine lets the program do, its execution state, follows from that state, the
//! engine's clock and whatever inhibits its freeze, by the one rule in `execution_state`. A part
//! of the engine that knows the program must keep running takes a freeze inhibitor for as long as
//! that is true (`freeze_inhibitor`): whoever plays or records its sound, its capture session,
//! and whatever carries a request it has yet to answer, a request to close one of its views or a
//! view's going among them (`views`).
//!
//! The invariant: the state is never reported from outside. It follows the views' presence by
//! the rule in `state_of_views`, so a program with no view keeps the state it had, and `paused`
//! has one way in, the app suspending (`state`). The execution state is published from one place,
//! here, under the lock its facts are kept under, with the state read inside it, so whoever
//! applies the rule last sees the latest state. Nothing here knows who reads it: the program's
//! thread, which does not enter a frozen program's code (`running`), and whoever submits its GPU
//! work or keeps its hardware buffers read it at their own doors. What a request to end is, and
//! when it is decided, are in `exit`. What the user opened with the program is news kept until
//! heard (`open_requests`): a request told while the program has no stream of them open waits for
//! its next, and inhibits its freeze while it is carried. The interface is served to a program by
//! `host`.

mod execution_state;
mod exit;
mod freeze_inhibitor;
pub(crate) mod host;
mod open_requests;
mod state;
mod state_of_views;

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::channel::mpsc;
use tokio::sync::watch;

use crate::bindings::wasdk::platform::lifecycle::{AppExitResponse, LifecycleState};
use crate::engine_time::EngineClock;
use crate::news::{Inhibiting, KeptNews};
use crate::sync::locked;
pub(crate) use execution_state::ExecutionState;
pub use execution_state::FreezePolicy;
use execution_state::{FreezeFacts, execution_state};
use exit::ExitListeners;
pub use exit::ExitRequest;
pub(crate) use exit::ExitRequests;
use freeze_inhibitor::FreezeInhibitors;
pub(crate) use freeze_inhibitor::{FreezeInhibitor, FreezeInhibitorKind};
pub use open_requests::OpenRequest;
use state::CurrentState;
pub use state_of_views::Presence;
use state_of_views::state_of_views;

/// One program's lifecycle, as its views derive it, the engine keeps it and the program reads
/// it. Every clone reaches the same one.
#[derive(Clone)]
pub(crate) struct Lifecycle(Arc<Parts>);

struct Parts {
    state: CurrentState,
    exit_listeners: Mutex<ExitListeners>,
    /// What the user opened with the program, kept for its next stream while none is open.
    open_requests: KeptNews<OpenRequest>,
    freeze_policy: FreezePolicy,
    /// What the rule weighs beside the state and the clock. The execution state is published
    /// under this lock.
    freeze_tracking: Mutex<FreezeTracking>,
    execution_state: watch::Sender<ExecutionState>,
    /// The engine's clock, which the rule reads when the state moves.
    clock: EngineClock,
}

/// What the lifecycle keeps for its rule: the program's freeze inhibitors, and since when it has
/// been in its state.
struct FreezeTracking {
    inhibitors: FreezeInhibitors,
    state: LifecycleState,
    state_since: Instant,
    /// Since when it has been hidden or paused, one after the other counted as one stretch.
    out_of_sight_since: Instant,
}

impl Default for Lifecycle {
    /// A program frozen by the default policy, on real time.
    fn default() -> Lifecycle {
        Lifecycle::new(FreezePolicy::default(), EngineClock::real())
    }
}

impl Lifecycle {
    /// A program that starts `Resumed`, with no stream open, frozen as `freeze` says, by the
    /// time `clock` reads.
    pub(crate) fn new(freeze: FreezePolicy, clock: EngineClock) -> Lifecycle {
        Lifecycle(Arc::new(Parts {
            state: CurrentState::new(),
            exit_listeners: Mutex::default(),
            open_requests: KeptNews::default(),
            freeze_policy: freeze,
            freeze_tracking: Mutex::new(FreezeTracking {
                inhibitors: FreezeInhibitors::default(),
                state: LifecycleState::Resumed,
                state_since: clock.now(),
                out_of_sight_since: clock.now(),
            }),
            execution_state: watch::Sender::new(ExecutionState::Running),
            clock,
        }))
    }

    /// The program's state now.
    pub(crate) fn state(&self) -> LifecycleState {
        self.0.state.now()
    }

    /// Each change of the program's state from now on, on a stream of its own.
    pub(crate) fn state_changes(&self) -> mpsc::UnboundedReceiver<LifecycleState> {
        self.0.state.changes()
    }

    /// The program's views stand as `views` say: its state follows, by the rule of
    /// [`state_of_views`], and its streams hear it unless it was in that state already. A program
    /// with no view keeps its state. Its freeze follows the state.
    pub(crate) fn follow_views(&self, views: impl IntoIterator<Item = Presence>) {
        if let Some(state) = state_of_views(views) {
            self.0.state.of_views(state);
            self.publish_execution_state();
        }
    }

    /// The app is suspending: the program is `paused`, and frozen at once.
    pub(crate) fn app_suspending(&self) {
        self.0.state.app_suspending();
        self.publish_execution_state();
    }

    /// The app resumed: the program is back in the state its views give, and unfrozen if that
    /// shows it.
    pub(crate) fn app_resumed(&self) {
        self.0.state.app_resumed();
        self.publish_execution_state();
    }

    /// The requests to end the program from now on, on a stream of their own.
    pub(crate) fn exit_requests(&self) -> ExitRequests {
        locked(&self.0.exit_listeners).listen()
    }

    /// Asks the program whether it may end: every stream open receives a request, even while an
    /// earlier one is undecided. Answers once every stream has answered, and at once when none is
    /// open: cancel if any cancelled, exit otherwise. The request inhibits the program's freeze
    /// until it has answered, or the question is let go.
    pub(crate) fn request_exit(
        &self,
    ) -> impl Future<Output = AppExitResponse> + Send + 'static + use<> {
        let pending = self.inhibit_freeze(FreezeInhibitorKind::PendingRequest);
        let answer = locked(&self.0.exit_listeners).ask();
        async move {
            let answer = answer.await;
            drop(pending);
            answer
        }
    }

    /// The user opened `request`'s files and links with the program: every stream of its open
    /// requests hears it, or, while none is open, the next one the program opens; its freeze
    /// inhibited until it has read it on each.
    pub(crate) fn open_requested(&self, request: OpenRequest) {
        self.0.open_requests.tell(request, self);
    }

    /// What the user opens with the program from now on, on a stream of its own, after what
    /// waited for one.
    pub(crate) fn open_requests(&self) -> mpsc::UnboundedReceiver<Inhibiting<OpenRequest>> {
        self.0.open_requests.subscribe(self)
    }

    /// The clock reads `now`: the program is frozen if it has been hidden for the policy's
    /// delay.
    pub(crate) fn freeze_if_due(&self, now: Instant) {
        self.publish_execution_state_with(now, |_| {});
    }

    /// Prevents the program's freeze until what is answered is dropped: a frozen program is
    /// unfrozen at once.
    pub(crate) fn inhibit_freeze(&self, kind: FreezeInhibitorKind) -> FreezeInhibitor {
        self.publish_execution_state_with(self.0.clock.now(), |tracking| {
            tracking.inhibitors.add(kind);
        });
        FreezeInhibitor::counted(kind, self.clone())
    }

    /// An inhibitor of `kind` was dropped: the rule applies to the program without it.
    fn freeze_inhibitor_dropped(&self, kind: FreezeInhibitorKind) {
        let now = self.0.clock.now();
        self.publish_execution_state_with(now, |tracking| tracking.inhibitors.remove(kind, now));
    }

    /// The program's execution state now, and each change of it: what every part of the engine
    /// that treats a frozen program differently reads. Whoever holds what `borrow` answers holds
    /// the state as it is, since a change waits for it: how GPU work is kept from starting once a
    /// freeze has taken.
    pub(crate) fn execution_state(&self) -> watch::Receiver<ExecutionState> {
        self.0.execution_state.subscribe()
    }

    /// Whether the program is frozen.
    pub(crate) fn is_frozen(&self) -> bool {
        *self.0.execution_state.borrow() == ExecutionState::Frozen
    }

    fn publish_execution_state(&self) {
        self.publish_execution_state_with(self.0.clock.now(), |_| {});
    }

    /// Publishes what the rule decides, after `adjust` changed what it weighs, with the state as
    /// it is now and the clock at `now`. The state is read under the lock, so the last
    /// application always sees the latest state.
    fn publish_execution_state_with(&self, now: Instant, adjust: impl FnOnce(&mut FreezeTracking)) {
        let mut tracking = locked(&self.0.freeze_tracking);
        adjust(&mut tracking);
        let state = self.0.state.now();
        if tracking.state != state {
            if in_sight(tracking.state) {
                tracking.out_of_sight_since = now;
            }
            tracking.state = state;
            tracking.state_since = now;
        }
        let since = |moment: Instant| now.saturating_duration_since(moment);
        let facts = FreezeFacts {
            state,
            in_state_for: since(tracking.state_since),
            out_of_sight_for: since(tracking.out_of_sight_since),
            inhibited: !tracking.inhibitors.is_empty(),
            uninhibited_for: tracking.inhibitors.last_dropped().map(since),
        };
        let decided = execution_state(facts, &self.0.freeze_policy);
        self.0.execution_state.send_if_modified(|published| {
            let changed = *published != decided;
            *published = decided;
            changed
        });
    }
}

/// Whether a program in `state` has a view on screen.
fn in_sight(state: LifecycleState) -> bool {
    matches!(state, LifecycleState::Resumed | LifecycleState::Inactive)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures::FutureExt;

    use super::*;

    const FOCUSED: Presence = Presence {
        on_screen: true,
        occluded: false,
        focused: true,
    };
    const OFF_SCREEN: Presence = Presence {
        on_screen: false,
        occluded: false,
        focused: false,
    };
    const DELAY: Duration = Duration::from_secs(5);

    fn lifecycle() -> Lifecycle {
        let policy = FreezePolicy {
            after_hidden: Some(DELAY),
        };
        Lifecycle::new(policy, EngineClock::real())
    }

    #[test]
    fn the_state_follows_the_views_and_is_heard_once() {
        let lifecycle = Lifecycle::default();
        let mut states = lifecycle.state_changes();
        lifecycle.follow_views([OFF_SCREEN]);
        lifecycle.follow_views([OFF_SCREEN]);
        lifecycle.follow_views([OFF_SCREEN, FOCUSED]);
        assert_eq!(lifecycle.state(), LifecycleState::Resumed);
        assert_eq!(states.try_recv().unwrap(), LifecycleState::Hidden);
        assert_eq!(states.try_recv().unwrap(), LifecycleState::Resumed);
        assert!(states.try_recv().is_err(), "each change is heard once");
    }

    #[test]
    fn a_program_with_no_view_keeps_its_state() {
        let lifecycle = Lifecycle::default();
        lifecycle.follow_views([OFF_SCREEN]);
        lifecycle.follow_views([]);
        assert_eq!(lifecycle.state(), LifecycleState::Hidden);
    }

    #[test]
    fn a_hidden_program_is_frozen_by_the_clock_and_unfrozen_when_shown() {
        let lifecycle = lifecycle();
        let before = Instant::now();
        lifecycle.follow_views([OFF_SCREEN]);
        lifecycle.freeze_if_due(before + DELAY - Duration::from_millis(1));
        assert!(!lifecycle.is_frozen(), "not before the delay");
        lifecycle.freeze_if_due(Instant::now() + DELAY);
        assert!(lifecycle.is_frozen());
        lifecycle.follow_views([FOCUSED]);
        assert!(!lifecycle.is_frozen(), "shown, it is unfrozen at once");
    }

    #[test]
    fn the_app_suspending_pauses_and_freezes_at_once_and_resuming_unfreezes_what_is_shown() {
        let lifecycle = lifecycle();
        let mut states = lifecycle.state_changes();
        lifecycle.app_suspending();
        assert_eq!(lifecycle.state(), LifecycleState::Paused);
        assert!(lifecycle.is_frozen());
        lifecycle.app_resumed();
        assert_eq!(lifecycle.state(), LifecycleState::Resumed);
        assert!(!lifecycle.is_frozen());
        let heard: Vec<_> = std::iter::from_fn(|| states.try_recv().ok()).collect();
        assert_eq!(heard, [LifecycleState::Paused, LifecycleState::Resumed]);
    }

    #[test]
    fn a_hidden_programs_delay_runs_through_a_suspension() {
        let lifecycle = lifecycle();
        lifecycle.follow_views([OFF_SCREEN]);
        lifecycle.app_suspending();
        assert!(lifecycle.is_frozen(), "paused, it is frozen at once");
        lifecycle.app_resumed();
        assert_eq!(lifecycle.state(), LifecycleState::Hidden);
        assert!(!lifecycle.is_frozen(), "hidden for less than the delay");
        lifecycle.freeze_if_due(Instant::now() + DELAY);
        assert!(
            lifecycle.is_frozen(),
            "hidden for the delay, suspension and all"
        );
    }

    #[test]
    fn a_frozen_program_is_unfrozen_to_answer_and_frozen_again_once_it_has() {
        let lifecycle = lifecycle();
        let mut requests = lifecycle.exit_requests();
        lifecycle.app_suspending();
        assert!(lifecycle.is_frozen());
        let mut answer = lifecycle.request_exit().boxed();
        assert!(!lifecycle.is_frozen(), "unfrozen to answer");
        assert_eq!((&mut answer).now_or_never(), None);
        requests
            .try_recv()
            .expect("asked")
            .respond(AppExitResponse::Cancel);
        assert_eq!(answer.now_or_never(), Some(AppExitResponse::Cancel));
        assert!(lifecycle.is_frozen(), "answered, and still paused");
    }

    #[test]
    fn a_question_let_go_applies_the_rule_again() {
        let lifecycle = lifecycle();
        let _requests = lifecycle.exit_requests();
        lifecycle.app_suspending();
        let answer = lifecycle.request_exit();
        assert!(!lifecycle.is_frozen());
        drop(answer);
        assert!(lifecycle.is_frozen(), "nobody waits for the answer");
    }

    #[test]
    fn the_execution_state_is_published_as_it_changes() {
        let lifecycle = lifecycle();
        let mut published = lifecycle.execution_state();
        assert_eq!(*published.borrow_and_update(), ExecutionState::Running);
        lifecycle.app_suspending();
        assert_eq!(published.has_changed().ok(), Some(true));
        assert_eq!(*published.borrow_and_update(), ExecutionState::Frozen);
        lifecycle.app_suspending();
        assert_eq!(
            published.has_changed().ok(),
            Some(false),
            "frozen already, nothing is published again"
        );
    }

    #[test]
    fn an_inhibitor_unfreezes_a_frozen_program_until_it_is_dropped() {
        let lifecycle = lifecycle();
        lifecycle.app_suspending();
        assert!(lifecycle.is_frozen());
        let audible = lifecycle.inhibit_freeze(FreezeInhibitorKind::AudioPlayback);
        assert!(!lifecycle.is_frozen(), "unfrozen at once");
        drop(audible);
        assert!(lifecycle.is_frozen(), "silent, and still paused");
    }

    /// A program's thread derives the state and the engine's threads ask through it.
    #[test]
    fn a_lifecycle_crosses_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<Lifecycle>();
    }
}
