//! Freezing under the test shell, on time the test moves (`EngineTime::Manual`) rather than real
//! time: `examples/forever` stops counting while it is frozen and goes on from the same count
//! after the thaw; a program hidden past the policy's delay freezes and one shown again before it
//! is over does not; one a tool watches does not freeze for being covered, but does once
//! minimized, and once the watch ends; `app_suspending` resolves once every program has stopped, and `app_resumed`
//! lets them go on; `examples/survey`, asked to exit while frozen, answers; frozen, it hears its
//! last view taken away and ends, and hears a request to close its view, since both decide whether
//! a program goes on; and `stop` ends a frozen program.
//!
//! The engine freezes a program [`AFTER_HIDDEN`] out of sight, at the first tick of its time that
//! finds the delay over, and a test moves the time past it, a tick at a time, instead of waiting.
//! A frozen program stops at its next epoch check, before more of its code runs: wasmtime checks
//! the epoch right after a fuel yield too. So once a freeze has taken, forever, which only runs,
//! stands at its freeze at its next check; survey, which waits in a host call, stands there once
//! an event has woken it and it has reached one. A test sees that through
//! `RunningProgram::stopped_at_freeze` rather than watching for a while. Two programs of one name
//! would print alike, so a test takes the turn of each program it runs (`shell::turns`).

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::bindings::wasdk::platform::views::Metrics;
use waos::{
    FreezePolicy, ManualTime, Presence, ProgramEnd, ProgramEnvironment, ProgramView,
    RunningProgram, WaOS,
};

use crate::shell::{
    self, Output, TestShell, metrics, output, resolved, wait_until_stopped_at_freeze,
};

/// How long a program stays hidden before the engine freezes it, in these tests: three ticks.
/// Time is moved by whole ticks from the engine's start, so a freeze due at a tick takes at it.
const AFTER_HIDDEN: Duration = Duration::from_millis(300);

/// The engine's tick, at which a freeze takes.
const TICK: Duration = Duration::from_millis(100);

/// How many steps forever counts between two reports, as `examples/forever` says.
const REPORT_EVERY: u64 = 1 << 20;

const OFF_SCREEN: Presence = Presence {
    on_screen: false,
    occluded: false,
    focused: false,
};
const FOCUSED: Presence = Presence {
    on_screen: true,
    occluded: false,
    focused: true,
};

#[test]
fn a_program_hidden_past_the_delay_stops_counting_and_goes_on_from_the_same_count_when_shown() {
    let _forever = shell::turns::take("forever-shown");
    let output = output();
    let (engine, shell, _arrivals, time) = engine();
    let (running, view) = run_forever_shown(&engine, &shell);
    let start = output.mark();
    first_report(output, start);

    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN - TICK);
    first_report(output, output.mark());
    time.elapse(TICK);
    wait_until_stopped_at_freeze(&running);
    let last = *reports_since(output, start)
        .last()
        .expect("it reported before it stopped");
    let mark = output.mark();
    time.elapse(AFTER_HIDDEN);
    wait_until_stopped_at_freeze(&running);
    assert!(
        reports_since(output, mark).is_empty(),
        "frozen, it counts nothing"
    );

    view.presence_changed(FOCUSED);
    assert_eq!(
        first_report(output, mark),
        last + REPORT_EVERY,
        "thawed, it goes on from where it stopped"
    );
    running.stop();
    assert!(matches!(resolved(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_program_shown_again_before_the_delay_is_over_is_not_frozen() {
    let _forever = shell::turns::take("forever-shown");
    let output = output();
    let (engine, shell, _arrivals, time) = engine();
    let (running, view) = run_forever_shown(&engine, &shell);
    first_report(output, output.mark());

    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN - TICK);
    view.presence_changed(FOCUSED);
    time.elapse(AFTER_HIDDEN * 2);
    first_report(output, output.mark());
    running.stop();
    assert!(matches!(resolved(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_watched_program_covered_past_the_delay_counts_on_and_freezes_once_minimized() {
    let _forever = shell::turns::take("forever-shown");
    let output = output();
    let (engine, shell, _arrivals, time) = engine();
    let (running, view) = run_forever_shown(&engine, &shell);
    first_report(output, output.mark());

    let watching = running.watch();
    shell::cover(&view);
    time.elapse(AFTER_HIDDEN * 2);
    first_report(output, output.mark());

    shell::hide(&view);
    time.elapse(AFTER_HIDDEN);
    wait_until_stopped_at_freeze(&running);
    drop(watching);
    running.stop();
    assert!(matches!(resolved(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_covered_program_freezes_once_its_watch_ends() {
    let _forever = shell::turns::take("forever-shown");
    let output = output();
    let (engine, shell, _arrivals, time) = engine();
    let (running, view) = run_forever_shown(&engine, &shell);
    first_report(output, output.mark());

    let watching = running.watch();
    shell::cover(&view);
    time.elapse(AFTER_HIDDEN * 2);
    first_report(output, output.mark());

    drop(watching);
    time.elapse(AFTER_HIDDEN);
    wait_until_stopped_at_freeze(&running);
    running.stop();
    assert!(matches!(resolved(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn the_app_suspending_resolves_once_every_program_has_stopped_and_resuming_lets_them_go_on() {
    let _forever = shell::turns::take("forever-shown");
    let _survey = shell::turns::take("survey");
    let output = output();
    let (engine, shell, _arrivals, time) = engine();
    let (forever, _) = run_forever_shown(&engine, &shell);
    let survey = shell::load(&engine, waos_test_programs::SURVEY);
    let (survey, _view) =
        shell::run_on_reserved(&engine, &shell, &survey, ProgramEnvironment::default());
    let mark = output.mark();
    first_report(output, mark);
    output.wait_for(mark, "survey", |line| line.starts_with("view 1: 1600x1000"));

    resolved(engine.app_suspending());
    // Survey hears it is paused, which wakes it, and stops at its epoch check.
    wait_until_stopped_at_freeze(&survey);
    let mark = output.mark();
    time.elapse(AFTER_HIDDEN);
    wait_until_stopped_at_freeze(&forever);
    wait_until_stopped_at_freeze(&survey);
    assert!(
        reports_since(output, mark).is_empty(),
        "suspended, forever counts nothing"
    );
    assert_eq!(
        output.lines_of(mark, "survey"),
        Vec::<String>::new(),
        "suspended, survey says nothing"
    );

    engine.app_resumed();
    first_report(output, mark);
    output.wait_for(mark, "survey", |line| {
        line == "program: LifecycleState::Resumed"
    });
    assert_eq!(
        output.lines_of(mark, "survey"),
        [
            "program: LifecycleState::Paused",
            "program: LifecycleState::Resumed"
        ],
        "a program hears it was paused, and resumed, once it runs again"
    );
    forever.stop();
    survey.stop();
    assert!(matches!(resolved(forever.ended()), ProgramEnd::Stopped));
    assert!(matches!(resolved(survey.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_program_started_while_the_app_is_suspended_starts_frozen() {
    let _forever = shell::turns::take("forever-shown");
    let output = output();
    let (engine, shell, _arrivals, time) = engine();
    resolved(engine.app_suspending());
    let (running, _view) = run_forever_shown(&engine, &shell);
    // Frozen before its code runs, it runs until its first tick, which comes once it counts.
    first_report(output, output.mark());
    time.elapse(TICK);
    wait_until_stopped_at_freeze(&running);
    let mark = output.mark();
    time.elapse(AFTER_HIDDEN);
    wait_until_stopped_at_freeze(&running);
    assert!(reports_since(output, mark).is_empty(), "it starts frozen");
    engine.app_resumed();
    first_report(output, mark);
    running.stop();
    assert!(matches!(resolved(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_program_asked_to_exit_while_frozen_answers_and_is_frozen_again_once_it_has() {
    let _survey = shell::turns::take("survey");
    let output = output();
    let (engine, shell, _arrivals, time) = engine();
    let survey = shell::load(&engine, waos_test_programs::SURVEY);
    let (running, view) =
        shell::run_on_reserved(&engine, &shell, &survey, ProgramEnvironment::default());
    let view = &view;
    let mark = output.mark();
    output.wait_for(mark, "survey", |line| line.starts_with("view 1: 1600x1000"));

    view.presence_changed(OFF_SCREEN);
    output.wait_for(mark, "survey", |line| {
        line == "program: LifecycleState::Hidden"
    });
    let mark = output.mark();
    freeze_then_change(&running, &time, view, metrics(800, 600));
    assert_eq!(
        output.lines_of(mark, "survey"),
        Vec::<String>::new(),
        "frozen, it does not hear its view change"
    );

    assert_eq!(
        exit_requested(&running),
        AppExitResponse::Exit,
        "thawed to answer, it answers"
    );
    let heard = output.lines_of(mark, "survey");
    assert!(
        heard.contains(&"program: asked whether it may end: yes".to_owned())
            && heard.contains(&"view 1: now 800x600".to_owned()),
        "thawed, it answers and hears what it missed: {heard:?}"
    );

    let mark = output.mark();
    freeze_then_change(&running, &time, view, metrics(640, 480));
    assert_eq!(
        output.lines_of(mark, "survey"),
        Vec::<String>::new(),
        "answered and still hidden, it is frozen again"
    );
    view.presence_changed(FOCUSED);
    output.wait_for(mark, "survey", |line| line == "view 1: now 640x480");
    running.stop();
    assert!(matches!(resolved(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_frozen_program_whose_last_view_is_taken_away_hears_it_and_ends() {
    let _survey = shell::turns::take("survey");
    let output = output();
    let (engine, shell, _arrivals, _time) = engine();
    let (running, view) = run_survey_shown(&engine, &shell, output);
    resolved(engine.app_suspending());
    wait_until_stopped_at_freeze(&running);

    let mark = output.mark();
    running.remove_view(view.id());
    assert!(
        matches!(resolved(running.ended()), ProgramEnd::Returned(Ok(()))),
        "thawed to hear its view go, it ends"
    );
    assert!(
        output
            .lines_of(mark, "survey")
            .contains(&"view 1 removed".to_owned())
    );
}

#[test]
fn a_frozen_program_asked_to_close_its_view_hears_it_and_is_frozen_again_once_it_has() {
    let _survey = shell::turns::take("survey");
    let output = output();
    let (engine, shell, _arrivals, _time) = engine();
    let (running, view) = run_survey_shown(&engine, &shell, output);
    resolved(engine.app_suspending());
    wait_until_stopped_at_freeze(&running);

    let mark = output.mark();
    view.close_requested();
    output.wait_for(mark, "survey", |line| line == "view 1: asked to close");
    // Back in a host call for its next event, it is frozen again, and stops once it runs.
    resolved(running.frozen());
    running.stop();
    assert!(matches!(resolved(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn stop_ends_a_frozen_program() {
    let _forever = shell::turns::take("forever-shown");
    let output = output();
    let (engine, shell, _arrivals, time) = engine();
    let (running, view) = run_forever_shown(&engine, &shell);
    first_report(output, output.mark());
    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN);
    wait_until_stopped_at_freeze(&running);

    running.stop();
    assert!(
        matches!(resolved(running.ended()), ProgramEnd::Stopped),
        "a frozen program ends when stopped, without a thaw"
    );
}

/// An engine freezing a program [`AFTER_HIDDEN`] out of sight, on time the test moves.
fn engine() -> (
    Arc<WaOS>,
    Arc<TestShell>,
    mpsc::Receiver<ProgramView>,
    ManualTime,
) {
    let time = ManualTime::new();
    let (engine, shell, arrivals) = shell::engine_on(
        &time,
        FreezePolicy {
            after_hidden: Some(AFTER_HIDDEN),
        },
    );
    (engine, shell, arrivals, time)
}

/// Forever-shown, running with one view the shell reserved for it, on screen and focused.
fn run_forever_shown(engine: &WaOS, shell: &TestShell) -> (RunningProgram, ProgramView) {
    let forever = shell::load(engine, waos_test_programs::FOREVER_SHOWN);
    shell::run_on_reserved(engine, shell, &forever, ProgramEnvironment::default())
}

/// Survey, running with one view the shell reserved for it, once it has described the view.
fn run_survey_shown(
    engine: &WaOS,
    shell: &TestShell,
    output: &Output,
) -> (RunningProgram, ProgramView) {
    let survey = shell::load(engine, waos_test_programs::SURVEY);
    let mark = output.mark();
    let shown = shell::run_on_reserved(engine, shell, &survey, ProgramEnvironment::default());
    output.wait_for(mark, "survey", |line| line.starts_with("view 1: 1600x1000"));
    shown
}

/// Moves time past the delay of survey, hidden and waiting for its events, which freezes it;
/// then changes its view's metrics to `changed`, which wakes it, and waits until it stands at its
/// freeze: at the epoch check it reaches before it can hear of the change.
fn freeze_then_change(
    survey: &RunningProgram,
    time: &ManualTime,
    view: &ProgramView,
    changed: Metrics,
) {
    time.elapse(AFTER_HIDDEN);
    view.metrics_changed(changed);
    time.elapse(TICK);
    wait_until_stopped_at_freeze(survey);
}

/// The counts forever reported since `mark`.
fn reports_since(output: &Output, mark: usize) -> Vec<u64> {
    output
        .lines_of(mark, "forever-shown")
        .iter()
        .map(|line| count_of(line))
        .collect()
}

fn count_of(report: &str) -> u64 {
    report
        .strip_prefix("count ")
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("forever reports its count, not {report:?}"))
}

/// Waits for forever's first report since `mark`.
fn first_report(output: &Output, mark: usize) -> u64 {
    count_of(&output.wait_for(mark, "forever-shown", |line| line.starts_with("count ")))
}

/// Asks `program` whether it may end, and waits for the answer, which a program that cannot
/// answer never gives.
fn exit_requested(program: &RunningProgram) -> AppExitResponse {
    resolved(program.request_exit())
}
