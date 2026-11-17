//! When the engine freezes a program: the rule, and the policy that sets its delay.
//!
//! A program out of sight is frozen. One hidden, once it has stayed hidden for the policy's
//! delay, so that switching back and forth costs nothing and a program told it is hidden has time
//! to save what it must not lose; one paused, at once, since the platform itself went to the
//! background, where a phone's system stops the app soon. It is thawed as soon as it is shown
//! again, and while it owes an answer, to an exit request, a paste of a form it promised, or a
//! choice or an event on its menus, since a frozen program cannot answer; while it is audible or
//! recording, since a program making sound, or taking it in, feeds or follows it as it goes, which
//! a phone's system allows an app in the background too; and while it runs a camera, which it
//! follows as it goes, as it follows sound it records (the lead's answer of 2026-11-03), a phone's
//! system interrupting the camera itself in the background. Why, and what was measured: a frozen
//! program uses no CPU, and gives back the driver's GPU memory about a second later.
//!
//! The rule is applied with the program's state and the clock as they stand, as often as either
//! moves, and decides the same whoever applies it. The invariant: a program is frozen by this
//! rule alone, so a program shown, one that owes an answer, one audible or recording, or one
//! running a camera, is never frozen.

use std::future::Future;
use std::time::{Duration, Instant};

use crate::bindings::wasdk::platform::lifecycle::LifecycleState;

use super::freezer::Freezer;

/// When the engine freezes a program it keeps out of sight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreezePolicy {
    /// How long a program stays hidden before the engine freezes it on its own; none: only when
    /// the app is suspending, which is the default. A hidden program's window may be one the user
    /// can still reach and close, a card in Android's Recents, and a program frozen alone hears
    /// nothing of it until it is shown again.
    pub after_hidden: Option<Duration>,
}

/// One program's freeze and what decides it: the policy, the delay as it runs, and the answers
/// the program owes.
pub(super) struct FreezeRule {
    policy: FreezePolicy,
    freezer: Freezer,
    /// Since when the program has been hidden, while its delay runs.
    hidden_since: Option<Instant>,
    /// The answers the program owes and has not given.
    answers_owed: u32,
    /// The sounds the program makes or records now.
    sounds: u32,
    /// The cameras the program runs now: its capture sessions.
    captures: u32,
}

/// When the rule wants a program frozen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FreezeWanted {
    /// Not frozen: the program is shown, owes an answer, is audible or recording, or runs a
    /// camera.
    Never,
    /// Once it has been hidden for this long.
    AfterHidden(Duration),
    /// Now: the platform itself went to the background.
    AtOnce,
}

impl FreezeRule {
    pub(super) fn new(policy: FreezePolicy) -> FreezeRule {
        FreezeRule {
            policy,
            freezer: Freezer::default(),
            hidden_since: None,
            answers_owed: 0,
            sounds: 0,
            captures: 0,
        }
    }

    /// Whether the program is frozen, or will stop at its next epoch tick.
    pub(super) fn is_frozen(&self) -> bool {
        self.freezer.is_frozen()
    }

    /// Resolves once the program is frozen: [`Freezer::until_frozen`].
    pub(super) fn until_frozen(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.freezer.until_frozen()
    }

    /// Resolves once the program stands at an epoch tick for its freeze:
    /// [`Freezer::stopped_at_tick`].
    pub(super) fn stopped_at_tick(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.freezer.stopped_at_tick()
    }

    /// What the program's store does at an epoch tick: [`Freezer::at_tick`].
    pub(super) fn at_tick(&mut self) -> Option<impl Future<Output = ()> + Send + 'static + use<>> {
        self.freezer.at_tick()
    }

    /// A stop while the program is frozen, let go by the thaw: [`Freezer::stop`].
    pub(super) fn until_thawed(
        &mut self,
    ) -> Option<impl Future<Output = ()> + Send + 'static + use<>> {
        self.freezer.stop()
    }

    /// The program owes one more answer.
    pub(super) fn answer_owed(&mut self) {
        self.answers_owed += 1;
    }

    /// The program answered, or the question was let go.
    pub(super) fn answered(&mut self) {
        self.answers_owed -= 1;
    }

    /// The program makes one more sound, or records.
    pub(super) fn sound_began(&mut self) {
        self.sounds += 1;
    }

    /// One of the program's sounds ended, or it stopped recording.
    pub(super) fn sound_ended(&mut self) {
        self.sounds -= 1;
    }

    /// The program runs one more camera.
    pub(super) fn capture_began(&mut self) {
        self.captures += 1;
    }

    /// One of the program's cameras stopped for it.
    pub(super) fn capture_ended(&mut self) {
        self.captures -= 1;
    }

    /// Keeps the rule with the program in `state` and the clock at `now`: freezes it when the
    /// rule wants it frozen now, lets its delay run or run out when the rule wants it frozen
    /// after one, and thaws it when the rule does not want it frozen. Answers whether the
    /// program was frozen by this, rather than frozen already or not at all.
    pub(super) fn keep(&mut self, state: LifecycleState, now: Instant) -> bool {
        match self.wanted(state) {
            FreezeWanted::Never => {
                self.hidden_since = None;
                self.freezer.thaw();
                false
            }
            FreezeWanted::AtOnce => {
                self.hidden_since = None;
                self.freezer.freeze()
            }
            FreezeWanted::AfterHidden(delay) => self.freeze_once_hidden_for(delay, now),
        }
    }

    fn wanted(&self, state: LifecycleState) -> FreezeWanted {
        if self.answers_owed > 0 || self.sounds > 0 || self.captures > 0 {
            return FreezeWanted::Never;
        }
        match state {
            LifecycleState::Resumed | LifecycleState::Inactive => FreezeWanted::Never,
            LifecycleState::Hidden => match self.policy.after_hidden {
                Some(delay) => FreezeWanted::AfterHidden(delay),
                None => FreezeWanted::Never,
            },
            LifecycleState::Paused => FreezeWanted::AtOnce,
        }
    }

    /// A program frozen already stays so. Otherwise its delay runs from the first time it is
    /// found hidden, and it is frozen once the delay is over, which is when this answers true.
    fn freeze_once_hidden_for(&mut self, delay: Duration, now: Instant) -> bool {
        if self.freezer.is_frozen() {
            return false;
        }
        let since = *self.hidden_since.get_or_insert(now);
        if now.saturating_duration_since(since) < delay {
            return false;
        }
        self.hidden_since = None;
        self.freezer.freeze()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LifecycleState::{Hidden, Inactive, Paused, Resumed};

    const DELAY: Duration = Duration::from_secs(5);
    const A_MOMENT: Duration = Duration::from_millis(1);

    fn rule() -> (FreezeRule, Instant) {
        let policy = FreezePolicy {
            after_hidden: Some(DELAY),
        };
        (FreezeRule::new(policy), Instant::now())
    }

    #[test]
    fn the_default_policy_freezes_a_program_only_when_the_app_suspends() {
        assert_eq!(FreezePolicy::default().after_hidden, None);
        let mut rule = FreezeRule::new(FreezePolicy::default());
        let start = Instant::now();
        rule.keep(Hidden, start);
        rule.keep(Hidden, start + Duration::from_secs(3600));
        assert!(!rule.is_frozen(), "hidden for an hour");
        rule.keep(Paused, start + Duration::from_secs(3600));
        assert!(rule.is_frozen(), "the app suspending");
    }

    #[test]
    fn a_hidden_program_is_frozen_once_the_delay_is_over() {
        let (mut rule, start) = rule();
        rule.keep(Hidden, start);
        rule.keep(Hidden, start + DELAY - A_MOMENT);
        assert!(!rule.is_frozen(), "not before the delay");
        rule.keep(Hidden, start + DELAY);
        assert!(rule.is_frozen());
    }

    #[test]
    fn the_delay_runs_from_the_first_time_the_program_is_found_hidden() {
        let (mut rule, start) = rule();
        rule.keep(Hidden, start);
        rule.keep(Hidden, start + DELAY / 2);
        rule.keep(Hidden, start + DELAY);
        assert!(rule.is_frozen(), "found hidden again, the delay went on");
    }

    #[test]
    fn a_program_shown_again_before_the_delay_is_never_frozen() {
        let (mut rule, start) = rule();
        rule.keep(Hidden, start);
        rule.keep(Inactive, start + DELAY / 2);
        rule.keep(Inactive, start + DELAY * 2);
        assert!(!rule.is_frozen());
    }

    #[test]
    fn a_frozen_program_shown_again_is_thawed_at_once() {
        let (mut rule, start) = rule();
        rule.keep(Hidden, start);
        rule.keep(Hidden, start + DELAY);
        assert!(rule.is_frozen());
        rule.keep(Resumed, start + DELAY);
        assert!(!rule.is_frozen());
    }

    #[test]
    fn going_out_of_sight_again_starts_the_delay_again() {
        let (mut rule, start) = rule();
        rule.keep(Hidden, start);
        rule.keep(Resumed, start + DELAY / 2);
        rule.keep(Hidden, start + DELAY / 2);
        rule.keep(Hidden, start + DELAY + DELAY / 2 - A_MOMENT);
        assert!(!rule.is_frozen(), "the first delay was let go");
        rule.keep(Hidden, start + DELAY + DELAY / 2);
        assert!(rule.is_frozen());
    }

    #[test]
    fn a_paused_program_is_frozen_at_once() {
        let (mut rule, start) = rule();
        rule.keep(Paused, start);
        assert!(rule.is_frozen());
    }

    #[test]
    fn a_hidden_program_paused_before_the_delay_is_over_is_frozen_at_once_and_stays_so() {
        let (mut rule, start) = rule();
        rule.keep(Hidden, start);
        rule.keep(Paused, start + DELAY / 2);
        assert!(rule.is_frozen(), "frozen without waiting out the delay");
        rule.keep(Hidden, start + DELAY / 2);
        rule.keep(Hidden, start + DELAY * 3);
        assert!(rule.is_frozen(), "hidden again, it stays frozen");
    }

    #[test]
    fn a_program_is_thawed_while_it_answers_and_frozen_again_after() {
        let (mut rule, start) = rule();
        rule.keep(Hidden, start);
        rule.keep(Hidden, start + DELAY);
        rule.answer_owed();
        rule.keep(Hidden, start + DELAY);
        assert!(!rule.is_frozen(), "thawed to answer");
        rule.keep(Hidden, start + DELAY * 3);
        assert!(!rule.is_frozen(), "kept thawed while it answers");
        rule.answered();
        rule.keep(Hidden, start + DELAY * 3);
        rule.keep(Hidden, start + DELAY * 4 - A_MOMENT);
        assert!(!rule.is_frozen(), "the delay runs from the answer");
        rule.keep(Hidden, start + DELAY * 4);
        assert!(rule.is_frozen(), "frozen again, still hidden");
    }

    #[test]
    fn a_program_asked_twice_is_kept_thawed_until_it_answered_both() {
        let (mut rule, start) = rule();
        rule.keep(Paused, start);
        rule.answer_owed();
        rule.answer_owed();
        rule.answered();
        rule.keep(Paused, start);
        assert!(!rule.is_frozen(), "one question is still open");
        rule.answered();
        rule.keep(Paused, start);
        assert!(rule.is_frozen());
    }

    #[test]
    fn a_program_is_thawed_while_it_sounds_paused_too_and_frozen_again_once_silent() {
        let (mut rule, start) = rule();
        rule.keep(Paused, start);
        assert!(rule.is_frozen());
        rule.sound_began();
        rule.keep(Paused, start);
        assert!(
            !rule.is_frozen(),
            "thawed while it sounds, the app in the background"
        );
        rule.sound_began();
        rule.sound_ended();
        rule.keep(Paused, start + DELAY);
        assert!(!rule.is_frozen(), "one sound still plays");
        rule.sound_ended();
        rule.keep(Paused, start + DELAY);
        assert!(rule.is_frozen(), "silent, and paused");
    }

    #[test]
    fn a_program_is_thawed_while_it_runs_a_camera_paused_too_and_frozen_again_once_it_stops() {
        let (mut rule, start) = rule();
        rule.keep(Paused, start);
        rule.capture_began();
        rule.keep(Paused, start);
        assert!(!rule.is_frozen(), "thawed while it runs a camera");
        rule.keep(Hidden, start + DELAY * 2);
        assert!(!rule.is_frozen(), "hidden past the delay, it is not frozen");
        rule.capture_ended();
        rule.keep(Paused, start + DELAY * 2);
        assert!(rule.is_frozen(), "its camera stopped, and paused");
    }

    #[test]
    fn a_hidden_program_falls_silent_and_is_frozen_after_the_delay() {
        let (mut rule, start) = rule();
        rule.sound_began();
        rule.keep(Hidden, start);
        rule.keep(Hidden, start + DELAY * 2);
        assert!(!rule.is_frozen(), "audible, it is not frozen");
        rule.sound_ended();
        rule.keep(Hidden, start + DELAY * 2);
        rule.keep(Hidden, start + DELAY * 3 - A_MOMENT);
        assert!(!rule.is_frozen(), "the delay runs from the silence");
        rule.keep(Hidden, start + DELAY * 3);
        assert!(rule.is_frozen());
    }

    #[test]
    fn without_a_delay_a_hidden_program_is_never_frozen_and_a_paused_one_is() {
        let mut rule = FreezeRule::new(FreezePolicy { after_hidden: None });
        let start = Instant::now();
        rule.keep(Hidden, start);
        rule.keep(Hidden, start + Duration::from_secs(3600));
        assert!(!rule.is_frozen());
        rule.keep(Paused, start);
        assert!(rule.is_frozen());
        rule.keep(Hidden, start);
        assert!(
            !rule.is_frozen(),
            "the app resumed: nothing keeps it frozen"
        );
    }

    #[test]
    fn a_frozen_program_stops_at_its_tick_and_a_thawed_one_goes_on() {
        let (mut rule, start) = rule();
        assert!(rule.at_tick().is_none());
        rule.keep(Paused, start);
        assert!(rule.at_tick().is_some());
        rule.keep(Resumed, start);
        assert!(rule.at_tick().is_none());
    }
}
