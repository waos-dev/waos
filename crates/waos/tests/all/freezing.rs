//! Freezing under the test shell, on time the test moves (`EngineTime::Manual`) rather than real
//! time: `examples/forever-shown` stops counting while it is frozen and goes on from the same
//! count after the thaw; a program hidden past the policy's delay freezes and one shown again
//! before it is over does not; one a tool watches does not freeze for being covered, but does once
//! minimized, and once the watch ends; `app_suspending` resolves once every program has stopped,
//! and `app_resumed` lets them go on; `examples/survey`, asked to exit while frozen, answers;
//! frozen, it hears its last view taken away and ends, and hears a request to close its view, since
//! both decide whether a program goes on; and `stop` ends a frozen program.
//!
//! The engine freezes a program [`AFTER_HIDDEN`] out of sight, at the first tick of its time that
//! finds the delay over, and a test moves the time past it, a tick at a time, instead of waiting.
//! A frozen program stops at its next epoch check, before more of its code runs: wasmtime checks
//! the epoch right after a fuel yield too. So once a freeze has taken, forever, which only runs,
//! stands at its freeze at its next check; survey, which waits in a host call, stands there once
//! an event has woken it and it has reached one. A test sees that through
//! `RunningProgram::stopped_at_freeze` rather than watching for a while. Each test reads only its
//! own programs' outputs.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::bindings::wasdk::platform::views::Metrics;
use waos::{
    EngineTime, FreezePolicy, ManualTime, Presence, ProgramEnd, ProgramView, RunningProgram, WaOS,
    WaOSConfig,
};
use waos_headless::{Display, HeadlessShell, Refreshes};

use crate::test_shell::{DISPLAY, RecordedOutput, TestShell, config, metrics, printing_to, within};

/// How long a program stays hidden before the engine freezes it, in these tests: three ticks.
/// Time is moved by whole ticks from the engine's start, so a freeze due at a tick takes at it.
const AFTER_HIDDEN: Duration = Duration::from_millis(300);

/// The engine's tick, at which a freeze takes.
const TICK: Duration = Duration::from_millis(100);

/// How many steps forever counts between two reports, as `examples/forever-shown` says.
const REPORT_EVERY: u64 = 1 << 20;

const OFF_SCREEN: Presence = Presence {
    on_screen: false,
    occluded: false,
    focused: false,
};
const COVERED: Presence = Presence {
    on_screen: true,
    occluded: true,
    focused: false,
};
const FOCUSED: Presence = Presence {
    on_screen: true,
    occluded: false,
    focused: true,
};

#[test]
fn a_program_hidden_past_the_delay_stops_counting_and_goes_on_from_the_same_count_when_shown() {
    let (engine, shell, time) = engine();
    let (running, view, output) = run_forever_shown(&engine, &shell, 0);
    first_report(&output, 0);

    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN - TICK);
    first_report(&output, output.lines().len());
    time.elapse(TICK);
    within(running.stopped_at_freeze());
    let last = *reports_since(&output, 0)
        .last()
        .expect("it reported before it stopped");
    let mark = output.lines().len();
    time.elapse(AFTER_HIDDEN);
    within(running.stopped_at_freeze());
    assert!(
        reports_since(&output, mark).is_empty(),
        "frozen, it counts nothing"
    );

    view.presence_changed(FOCUSED);
    assert_eq!(
        first_report(&output, mark),
        last + REPORT_EVERY,
        "thawed, it goes on from where it stopped"
    );
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_program_shown_again_before_the_delay_is_over_is_not_frozen() {
    let (engine, shell, time) = engine();
    let (running, view, output) = run_forever_shown(&engine, &shell, 0);
    first_report(&output, 0);

    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN - TICK);
    view.presence_changed(FOCUSED);
    time.elapse(AFTER_HIDDEN * 2);
    first_report(&output, output.lines().len());
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_watched_program_covered_past_the_delay_counts_on_and_freezes_once_minimized() {
    let (engine, shell, time) = engine();
    let (running, view, output) = run_forever_shown(&engine, &shell, 0);
    first_report(&output, 0);

    let watching = running.watch();
    view.presence_changed(COVERED);
    time.elapse(AFTER_HIDDEN * 2);
    first_report(&output, output.lines().len());

    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN);
    within(running.stopped_at_freeze());
    drop(watching);
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_covered_program_freezes_once_its_watch_ends() {
    let (engine, shell, time) = engine();
    let (running, view, output) = run_forever_shown(&engine, &shell, 0);
    first_report(&output, 0);

    let watching = running.watch();
    view.presence_changed(COVERED);
    time.elapse(AFTER_HIDDEN * 2);
    first_report(&output, output.lines().len());

    drop(watching);
    time.elapse(AFTER_HIDDEN);
    within(running.stopped_at_freeze());
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn the_app_suspending_resolves_once_every_program_has_stopped_and_resuming_lets_them_go_on() {
    let (engine, shell, time) = engine();
    let (forever, _, forever_output) = run_forever_shown(&engine, &shell, 0);
    let (survey, _, survey_output) = run_survey_shown(&engine, &shell, 1);
    first_report(&forever_output, 0);

    within(engine.app_suspending());
    // Survey hears it is paused, which wakes it, and stops at its epoch check.
    within(survey.stopped_at_freeze());
    let forever_mark = forever_output.lines().len();
    let survey_mark = survey_output.lines().len();
    time.elapse(AFTER_HIDDEN);
    within(forever.stopped_at_freeze());
    within(survey.stopped_at_freeze());
    assert!(
        reports_since(&forever_output, forever_mark).is_empty(),
        "suspended, forever counts nothing"
    );
    assert_eq!(
        &survey_output.lines()[survey_mark..],
        Vec::<String>::new(),
        "suspended, survey says nothing"
    );

    engine.app_resumed();
    first_report(&forever_output, forever_mark);
    survey_output.wait_for("program: LifecycleState::Resumed");
    assert_eq!(
        &survey_output.lines()[survey_mark..],
        [
            "program: LifecycleState::Paused",
            "program: LifecycleState::Resumed"
        ],
        "a program hears it was paused, and resumed, once it runs again"
    );
    forever.stop();
    survey.stop();
    assert!(matches!(within(forever.ended()), ProgramEnd::Stopped));
    assert!(matches!(within(survey.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_program_started_while_the_app_is_suspended_starts_frozen() {
    let (engine, shell, time) = engine();
    within(engine.app_suspending());
    let (running, _view, output) = run_forever_shown(&engine, &shell, 0);
    // Frozen before its code runs, it runs until its first tick, which comes once it counts.
    first_report(&output, 0);
    time.elapse(TICK);
    within(running.stopped_at_freeze());
    let mark = output.lines().len();
    time.elapse(AFTER_HIDDEN);
    within(running.stopped_at_freeze());
    assert!(reports_since(&output, mark).is_empty(), "it starts frozen");
    engine.app_resumed();
    first_report(&output, mark);
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_program_asked_to_exit_while_frozen_answers_and_is_frozen_again_once_it_has() {
    let (engine, shell, time) = engine();
    let (running, view, output) = run_survey_shown(&engine, &shell, 0);

    view.presence_changed(OFF_SCREEN);
    output.wait_for("program: LifecycleState::Hidden");
    let mark = output.lines().len();
    freeze_then_change(&running, &time, &view, metrics(800, 600));
    assert_eq!(
        &output.lines()[mark..],
        Vec::<String>::new(),
        "frozen, it does not hear its view change"
    );

    assert_eq!(
        within(running.request_exit()),
        AppExitResponse::Exit,
        "thawed to answer, it answers"
    );
    let heard = &output.lines()[mark..];
    assert!(
        heard.contains(&"program: asked whether it may end: yes".to_owned())
            && heard.contains(&"view 1: now 800x600".to_owned()),
        "thawed, it answers and hears what it missed: {heard:?}"
    );

    let mark = output.lines().len();
    freeze_then_change(&running, &time, &view, metrics(640, 480));
    assert_eq!(
        &output.lines()[mark..],
        Vec::<String>::new(),
        "answered and still hidden, it is frozen again"
    );
    view.presence_changed(FOCUSED);
    output.wait_for("view 1: now 640x480");
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_frozen_program_whose_last_view_is_taken_away_hears_it_and_ends() {
    let (engine, shell, _time) = engine();
    let (running, view, output) = run_survey_shown(&engine, &shell, 0);
    within(engine.app_suspending());
    within(running.stopped_at_freeze());

    let mark = output.lines().len();
    running.remove_view(view.id());
    assert!(
        matches!(within(running.ended()), ProgramEnd::Returned(Ok(()))),
        "thawed to hear its view go, it ends"
    );
    assert!(output.lines()[mark..].contains(&"view 1 removed".to_owned()));
}

#[test]
fn a_frozen_program_asked_to_close_its_view_hears_it_and_is_frozen_again_once_it_has() {
    // Without a window, survey has nothing to close on a close request, so it returns to its
    // event loop and freezes again instead of ending with its view.
    let time = ManualTime::new();
    let mut shell = TestShell::new();
    shell.headless = HeadlessShell::new(
        Display {
            windows: false,
            ..DISPLAY
        },
        Refreshes::WhenAsked,
    );
    let engine = WaOS::new(
        WaOSConfig {
            freeze: FreezePolicy {
                after_hidden: Some(AFTER_HIDDEN),
            },
            time: EngineTime::Manual(time),
            ..config()
        },
        shell.shell(),
        shell.platform(),
    )
    .unwrap();
    let survey = engine.load(Path::new(waos_test_programs::SURVEY)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&survey, printing_to(&output)).unwrap();
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("survey asks for a view");
    output.wait_until("survey describes its view", |lines| {
        lines
            .iter()
            .any(|line| line.starts_with("view 1: 1600x1000"))
    });
    within(engine.app_suspending());
    within(running.stopped_at_freeze());

    view.close_requested();
    output.wait_for("view 1: asked to close");
    // Back in a host call for its next event, it is frozen again, and stops once it runs.
    within(running.frozen());
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn stop_ends_a_frozen_program() {
    let (engine, shell, time) = engine();
    let (running, view, output) = run_forever_shown(&engine, &shell, 0);
    first_report(&output, 0);
    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN);
    within(running.stopped_at_freeze());

    running.stop();
    assert!(
        matches!(within(running.ended()), ProgramEnd::Stopped),
        "a frozen program ends when stopped, without a thaw"
    );
}

/// An engine freezing a program [`AFTER_HIDDEN`] out of sight, on time the test moves.
fn engine() -> (WaOS, TestShell, ManualTime) {
    let time = ManualTime::new();
    let shell = TestShell::new();
    let engine = WaOS::new(
        WaOSConfig {
            freeze: FreezePolicy {
                after_hidden: Some(AFTER_HIDDEN),
            },
            time: EngineTime::Manual(time.clone()),
            ..config()
        },
        shell.shell(),
        shell.platform(),
    )
    .unwrap();
    (engine, shell, time)
}

/// Forever-shown, running with the view it asks for, on screen and focused, and its output.
fn run_forever_shown(
    engine: &WaOS,
    shell: &TestShell,
    view_index: usize,
) -> (RunningProgram, ProgramView, Arc<RecordedOutput>) {
    let forever = engine
        .load(Path::new(waos_test_programs::FOREVER_SHOWN))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&forever, printing_to(&output)).unwrap();
    let view = within(shell.headless.view(view_index))
        .and_then(|shown| shown.view())
        .expect("forever-shown asks for a view");
    (running, view, output)
}

/// Survey, running with the view it asks for, once it has finished its startup lines, and its
/// output.
fn run_survey_shown(
    engine: &WaOS,
    shell: &TestShell,
    view_index: usize,
) -> (RunningProgram, ProgramView, Arc<RecordedOutput>) {
    let survey = engine.load(Path::new(waos_test_programs::SURVEY)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&survey, printing_to(&output)).unwrap();
    let view = within(shell.headless.view(view_index))
        .and_then(|shown| shown.view())
        .expect("survey asks for a view");
    output.wait_until("survey describes its view", |lines| {
        lines
            .iter()
            .any(|line| line.starts_with("view 1: 1600x1000"))
    });
    output.wait_for("view 1: its window keeps between 320x240 and 1920x1200");
    (running, view, output)
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
    within(survey.stopped_at_freeze());
}

/// The counts forever reported since line `mark`.
fn reports_since(output: &RecordedOutput, mark: usize) -> Vec<u64> {
    output.lines()[mark..]
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

/// Waits for forever's first report since line `mark`.
fn first_report(output: &RecordedOutput, mark: usize) -> u64 {
    output.wait_until("forever reports its count", |lines| {
        lines[mark..].iter().any(|line| line.starts_with("count "))
    });
    output.lines()[mark..]
        .iter()
        .find(|line| line.starts_with("count "))
        .map(|line| count_of(line))
        .expect("a count report")
}
