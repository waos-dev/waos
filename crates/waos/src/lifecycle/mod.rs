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
//! A program is thawed while it is audible or recording, which whoever plays or records its sound
//! holds a [`Sounding`] for, as an answer owed is held; while it runs a camera, which its
//! capture session holds a [`Capturing`] for; and while what decides whether it goes on waits for
//! it to handle it, a request to close one of its views or a view's going, which the view's events
//! and the stream of changes among its views hold an answer owed for (`views`).
//!
//! The invariant: the state is never reported from outside. It follows the views' presence by
//! the rule in `state_of_views`, so a program with no view keeps the state it had, and `paused`
//! has one way in, the app suspending (`state`). The freeze follows the state by the rule in
//! `freeze_rule`, applied under its own lock with the state read inside it, so whoever applies it
//! last sees the latest state; a program is frozen by that rule alone, and what a freeze is, is
//! `freezer`. What a frozen program lets go of is not the lifecycle's to know: whoever lets go
//! asks to be run at each freeze (`when_frozen`), and whoever must not submit GPU work while
//! frozen waits on `until_thawed`. What a request to end is, and when it is decided, are in
//! `exit`. What the user opened with the program is news kept until heard (`open_requests`): a
//! request told while the program has no stream of them open waits for its next, and thaws it
//! while it is carried. The interface is served to a program by `host`.

mod exit;
mod freeze_rule;
mod freezer;
pub(crate) mod host;
mod open_requests;
mod state;
mod state_of_views;

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::channel::mpsc;

use crate::bindings::wasdk::platform::lifecycle::{AppExitResponse, LifecycleState};
use crate::engine_time::EngineClock;
use crate::news::{KeptNews, Thawing};
use crate::sync::locked;
use exit::ExitListeners;
pub use exit::ExitRequest;
pub(crate) use exit::ExitRequests;
pub use freeze_rule::FreezePolicy;
use freeze_rule::FreezeRule;
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
    freeze: Mutex<FreezeRule>,
    /// The engine's clock, which the rule reads when the state moves.
    clock: EngineClock,
    /// What runs at each freeze: what a frozen program lets go of.
    frozen_listeners: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
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
            freeze: Mutex::new(FreezeRule::new(freeze)),
            clock,
            frozen_listeners: Mutex::default(),
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
            self.keep_freeze_rule();
        }
    }

    /// The app is suspending: the program is `paused`, and frozen at once.
    pub(crate) fn app_suspending(&self) {
        self.0.state.app_suspending();
        self.keep_freeze_rule();
    }

    /// The app resumed: the program is back in the state its views give, and thawed if that
    /// shows it.
    pub(crate) fn app_resumed(&self) {
        self.0.state.app_resumed();
        self.keep_freeze_rule();
    }

    /// The requests to end the program from now on, on a stream of their own.
    pub(crate) fn exit_requests(&self) -> ExitRequests {
        locked(&self.0.exit_listeners).listen()
    }

    /// Asks the program whether it may end: every stream open receives a request, even while an
    /// earlier one is undecided. Answers once every stream has answered, and at once when none is
    /// open: cancel if any cancelled, exit otherwise. The program is thawed while it owes the
    /// answer, and its freeze follows the rule again once it has answered, or once the question
    /// is let go.
    pub(crate) fn request_exit(
        &self,
    ) -> impl Future<Output = AppExitResponse> + Send + 'static + use<> {
        let owed = self.owe_answer();
        let answer = locked(&self.0.exit_listeners).ask();
        async move {
            let answer = answer.await;
            drop(owed);
            answer
        }
    }

    /// The user opened `request`'s files and links with the program: every stream of its open
    /// requests hears it, or, while none is open, the next one the program opens; the program
    /// thawed until it has read it on each.
    pub(crate) fn open_requested(&self, request: OpenRequest) {
        self.0.open_requests.tell(request, self);
    }

    /// What the user opens with the program from now on, on a stream of its own, after what
    /// waited for one.
    pub(crate) fn open_requests(&self) -> mpsc::UnboundedReceiver<Thawing<OpenRequest>> {
        self.0.open_requests.subscribe(self)
    }

    /// The clock reads `now`: the program is frozen if it has been hidden for the policy's
    /// delay.
    pub(crate) fn freeze_if_due(&self, now: Instant) {
        self.keep_freeze_rule_at(now);
    }

    /// What the program's store does at an epoch tick: goes on (`None`), or, frozen, waits
    /// there until the thaw, which is when the future resolves.
    pub(crate) fn at_tick(&self) -> Option<impl Future<Output = ()> + Send + 'static + use<>> {
        locked(&self.0.freeze).at_tick()
    }

    /// Resolves at once while the program is not frozen, and at the thaw while it is: where a
    /// host call that would submit GPU work waits, since a platform may refuse GPU work from the
    /// background, and a program waiting in a host call when the app suspends could reach one
    /// before its first epoch check.
    pub(crate) fn until_thawed(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let stopped = locked(&self.0.freeze).until_thawed();
        async move {
            if let Some(stopped) = stopped {
                tracing::debug!("GPU work waits for the thaw");
                stopped.await;
            }
        }
    }

    /// Runs `work` unless the program is frozen, under the lock a freeze takes, so no freeze
    /// takes while it runs: a freeze waits the moment `work` takes. Answers whether it ran; a
    /// caller answered no waits for the thaw and asks again. For the submission of GPU work,
    /// which must not start once a freeze has taken. `work` takes no lock of the lifecycle's.
    pub(crate) fn while_thawed(&self, work: &mut dyn FnMut()) -> bool {
        let freeze = locked(&self.0.freeze);
        if freeze.is_frozen() {
            return false;
        }
        work();
        true
    }

    /// Resolves once the program is frozen, at once while it is.
    pub(crate) fn until_frozen(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        locked(&self.0.freeze).until_frozen()
    }

    /// Resolves once the program stands at an epoch tick for its freeze, at once while it does.
    pub(crate) fn stopped_at_freeze(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        locked(&self.0.freeze).stopped_at_tick()
    }

    /// Whether the program is frozen, or will stop at its next epoch tick.
    pub(crate) fn is_frozen(&self) -> bool {
        locked(&self.0.freeze).is_frozen()
    }

    /// Runs `then` at every freeze from now on, from the thread that freezes the program and
    /// outside the lifecycle's locks: where what a frozen program need not keep is let go.
    pub(crate) fn when_frozen(&self, then: Arc<dyn Fn() + Send + Sync>) {
        locked(&self.0.frozen_listeners).push(then);
    }

    /// The program owes one more answer, and is thawed until it has answered every one: to a
    /// request to end, to a paste of a form it promised, or to a choice or an event on its menus
    /// and tray icons, or a request to open, which it answers by handling it.
    pub(crate) fn owe_answer(&self) -> AnswerOwed {
        self.keep_freeze_rule_with(self.0.clock.now(), FreezeRule::answer_owed);
        AnswerOwed(self.clone())
    }

    /// The program answered, or the question was let go: the rule applies to it again.
    fn answered(&self) {
        self.keep_freeze_rule_with(self.0.clock.now(), FreezeRule::answered);
    }

    /// The program is audible or recording, and is thawed until every sound it makes or records
    /// has ended: until each [`Sounding`] is dropped. A frozen program that starts to sound, as one
    /// whose scheduled sound begins, is thawed at once.
    pub(crate) fn sounding(&self) -> Sounding {
        self.keep_freeze_rule_with(self.0.clock.now(), FreezeRule::sound_began);
        Sounding(self.clone())
    }

    /// One of the program's sounds ended: the rule applies to it again.
    fn sound_ended(&self) {
        self.keep_freeze_rule_with(self.0.clock.now(), FreezeRule::sound_ended);
    }

    /// The program runs a camera, and is thawed until every camera it runs has stopped for it:
    /// until each [`Capturing`] is dropped.
    pub(crate) fn capturing(&self) -> Capturing {
        self.keep_freeze_rule_with(self.0.clock.now(), FreezeRule::capture_began);
        Capturing(self.clone())
    }

    /// One of the program's cameras stopped for it: the rule applies to it again.
    fn capture_ended(&self) {
        self.keep_freeze_rule_with(self.0.clock.now(), FreezeRule::capture_ended);
    }

    fn keep_freeze_rule(&self) {
        self.keep_freeze_rule_at(self.0.clock.now());
    }

    fn keep_freeze_rule_at(&self, now: Instant) {
        self.keep_freeze_rule_with(now, |_| {});
    }

    /// Keeps the freeze rule, after `adjust` changed what it weighs, with the state as it is
    /// now and the clock at `now`. The state is read under the rule's lock, so the last
    /// application always sees the latest state; what a freeze lets go of runs after the lock.
    fn keep_freeze_rule_with(&self, now: Instant, adjust: impl FnOnce(&mut FreezeRule)) {
        let froze = {
            let mut freeze = locked(&self.0.freeze);
            adjust(&mut freeze);
            freeze.keep(self.0.state.now(), now)
        };
        if froze {
            self.frozen();
        }
    }

    /// The program was frozen just now: what it lets go of goes.
    fn frozen(&self) {
        let listeners = locked(&self.0.frozen_listeners).clone();
        for listener in listeners {
            listener();
        }
    }
}

/// An answer the program owes, held while it does. Dropped, the program has answered or the
/// question was let go, and the freeze rule applies to it again.
pub(crate) struct AnswerOwed(Lifecycle);

impl Drop for AnswerOwed {
    fn drop(&mut self) {
        self.0.answered();
    }
}

/// A sound the program makes or records, held while it does. Dropped, the sound has ended, and
/// the freeze rule applies to the program again.
pub(crate) struct Sounding(Lifecycle);

impl Drop for Sounding {
    fn drop(&mut self) {
        self.0.sound_ended();
    }
}

/// A camera the program runs, held while it does. Dropped, the camera has stopped for it, and the
/// freeze rule applies to the program again.
pub(crate) struct Capturing(Lifecycle);

impl Drop for Capturing {
    fn drop(&mut self) {
        self.0.capture_ended();
    }
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
    fn a_hidden_program_is_frozen_by_the_clock_and_thawed_when_shown() {
        let lifecycle = lifecycle();
        let before = Instant::now();
        lifecycle.follow_views([OFF_SCREEN]);
        lifecycle.freeze_if_due(before + DELAY - Duration::from_millis(1));
        assert!(!lifecycle.is_frozen(), "not before the delay");
        lifecycle.freeze_if_due(Instant::now() + DELAY);
        assert!(lifecycle.is_frozen());
        lifecycle.follow_views([FOCUSED]);
        assert!(!lifecycle.is_frozen(), "shown, it is thawed at once");
    }

    #[test]
    fn the_app_suspending_pauses_and_freezes_at_once_and_resuming_thaws_what_is_shown() {
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
    fn a_hidden_program_stays_frozen_when_the_app_resumes() {
        let lifecycle = lifecycle();
        lifecycle.follow_views([OFF_SCREEN]);
        lifecycle.app_suspending();
        lifecycle.app_resumed();
        assert_eq!(lifecycle.state(), LifecycleState::Hidden);
        assert!(lifecycle.is_frozen());
    }

    #[test]
    fn a_frozen_program_is_thawed_to_answer_and_frozen_again_once_it_has() {
        let lifecycle = lifecycle();
        let mut requests = lifecycle.exit_requests();
        lifecycle.app_suspending();
        assert!(lifecycle.is_frozen());
        let mut answer = lifecycle.request_exit().boxed();
        assert!(!lifecycle.is_frozen(), "thawed to answer");
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
    fn a_frozen_program_stops_at_its_tick_until_the_thaw() {
        let lifecycle = lifecycle();
        assert!(lifecycle.at_tick().is_none(), "it goes on while not frozen");
        lifecycle.app_suspending();
        let mut stopped = lifecycle.at_tick().expect("frozen, it stops").boxed();
        assert_eq!((&mut stopped).now_or_never(), None);
        lifecycle.app_resumed();
        assert_eq!(stopped.now_or_never(), Some(()));
    }

    #[test]
    fn work_runs_while_thawed_and_not_once_frozen() {
        let lifecycle = lifecycle();
        let mut ran = 0;
        assert!(lifecycle.while_thawed(&mut || ran += 1));
        lifecycle.app_suspending();
        assert!(
            !lifecycle.while_thawed(&mut || ran += 1),
            "frozen, it is refused"
        );
        assert_eq!(ran, 1);
        lifecycle.app_resumed();
        assert!(lifecycle.while_thawed(&mut || ran += 1));
        assert_eq!(ran, 2);
    }

    /// A freeze asked for while work runs takes only once the work is over.
    #[test]
    fn a_freeze_waits_for_work_running_while_thawed() {
        let lifecycle = lifecycle();
        let mut work_over_at = None;
        let mut frozen_at = None;
        let ran = lifecycle.while_thawed(&mut || {
            let freezing = lifecycle.clone();
            let freezer = std::thread::spawn(move || {
                freezing.app_suspending();
                Instant::now()
            });
            std::thread::sleep(Duration::from_millis(50));
            work_over_at = Some(Instant::now());
            // The freeze is still waiting here: joining it would wait on ourselves.
            frozen_at = Some(freezer);
        });
        assert!(ran);
        let frozen_at = frozen_at.unwrap().join().unwrap();
        assert!(
            frozen_at >= work_over_at.unwrap(),
            "the freeze took only once the work was over"
        );
        assert!(lifecycle.is_frozen());
    }

    #[test]
    fn a_frozen_program_that_sounds_is_thawed_until_its_sound_ends() {
        let lifecycle = lifecycle();
        lifecycle.app_suspending();
        assert!(lifecycle.is_frozen());
        let sounding = lifecycle.sounding();
        assert!(!lifecycle.is_frozen(), "thawed at once");
        drop(sounding);
        assert!(lifecycle.is_frozen(), "silent, and still paused");
    }

    /// A program's thread derives the state and the engine's threads ask through it.
    #[test]
    fn a_lifecycle_crosses_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<Lifecycle>();
    }
}
