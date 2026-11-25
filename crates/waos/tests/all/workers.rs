//! `examples/workers` under the test shell, each test reading the program's own output: a worker
//! at an entry the program does not have is refused as unknown; three words cross to a worker and
//! back, in order; a worker counts primes while the main instance goes on waking; a worker that
//! traps ends failed; a worker starts workers of its own; a worker asking for the user's side gets
//! none of it; a worker dropped mid-work ends; a program that ends, or is stopped, with a worker
//! running is told ended once the worker has ended too; and a frozen program's worker stops
//! counting until the unfreeze.
//!
//! The program runs its steps in order and prints each, and ends by starting a counting worker
//! and waiting to be asked to exit; a test reads the step it is about, then ends it.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::{
    EngineTime, FreezePolicy, ManualTime, Presence, ProgramEnd, RunningProgram, WaOS, WaOSConfig,
};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, wait_until, within};

/// The primes below 200,000, which the program counts on a worker.
const PRIMES_BELOW_200_000: u32 = 17_984;

/// What the program's last step says once its counting worker runs and it waits to be asked to
/// exit.
const COUNTING: &str = "counting: until asked to exit";

#[test]
fn an_entry_the_program_does_not_have_is_refused_as_unknown() {
    let (_engine, running, output) = run_workers();
    output.wait_for("unknown entry: nothing");
    end(&running);
}

#[test]
fn messages_cross_to_a_worker_and_back_in_order() {
    let (_engine, running, output) = run_workers();
    output.wait_for("reversed: eno owt eerht");
    output.wait_for("reverse finished: ok");
    end(&running);
}

#[test]
fn a_worker_computes_while_the_main_instance_goes_on() {
    let (_engine, running, output) = run_workers();
    let prefix = "primes below 200000: ";
    output.wait_until("the primes are counted", |lines| {
        lines.iter().any(|line| line.starts_with(prefix))
    });
    let line = output
        .lines()
        .into_iter()
        .find(|line| line.starts_with(prefix))
        .unwrap();
    let (count, wakes) = primes_and_wakes(&line[prefix.len()..]);
    assert_eq!(count, PRIMES_BELOW_200_000);
    assert!(
        wakes > 0,
        "the main instance woke while the worker computed: {line}"
    );
    end(&running);
}

#[test]
fn a_worker_that_traps_ends_failed() {
    let (_engine, running, output) = run_workers();
    output.wait_until("the trap is told", |lines| {
        lines
            .iter()
            .any(|line| line.starts_with("trap finished: failed"))
    });
    end(&running);
}

#[test]
fn a_worker_starts_workers_of_its_own() {
    let (_engine, running, output) = run_workers();
    output.wait_for("relayed: relay");
    output.wait_for("relay finished: ok");
    end(&running);
}

#[test]
fn a_worker_has_none_of_the_users_side() {
    let (_engine, running, output) = run_workers();
    output.wait_for(
        "user's side in a worker: views 0, request unsupported, compositor none, clipboard none, \
         menu bar none, file dialogs none, notifications none, opener none, audio none, media \
         players none, cameras none, exit requests end",
    );
    end(&running);
}

#[test]
fn a_worker_dropped_mid_work_ends() {
    let (_engine, running, output) = run_workers();
    output.wait_for("dropped: a counting worker");
    output.wait_for(COUNTING);
    wait_until("only the last step's worker runs", || {
        running.stats().workers() == 1
    });
    end(&running);
}

#[test]
fn a_program_that_ends_with_a_worker_running_is_told_ended_once_the_worker_has() {
    let (_engine, running, output) = run_workers();
    output.wait_for(COUNTING);
    assert_eq!(
        within(running.request_exit()),
        AppExitResponse::Exit,
        "the program answers exit"
    );
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    output.wait_for("exit: with a worker counting");
    assert_eq!(running.stats().workers(), 0);
}

#[test]
fn a_program_stopped_with_a_worker_running_is_told_stopped_once_the_worker_has_ended() {
    let (_engine, running, output) = run_workers();
    output.wait_for(COUNTING);
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
    assert_eq!(running.stats().workers(), 0);
}

#[test]
fn a_frozen_programs_worker_stops_counting_until_the_unfreeze() {
    let time = ManualTime::new();
    let shell = TestShell::new();
    let config = WaOSConfig {
        time: EngineTime::Manual(time.clone()),
        freeze: FreezePolicy {
            freeze_hidden_after: Some(AFTER_HIDDEN),
            ..FreezePolicy::default()
        },
        ..config()
    };
    let engine = WaOS::new(config, shell.shell(), shell.platform()).unwrap();
    let workers = engine.load(Path::new(waos_test_programs::WORKERS)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&workers, printing_to(&output)).unwrap();
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("workers asks for a view");
    output.wait_for(COUNTING);
    let first_count = counts(&output.lines());
    output.wait_until("the worker counts", |lines| counts(lines) > first_count);

    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN);
    within(running.frozen());
    let frozen_at = counts(&output.lines());
    time.elapse(AFTER_HIDDEN);
    within(running.frozen());
    assert_eq!(
        counts(&output.lines()),
        frozen_at,
        "frozen, the worker counts nothing"
    );

    view.presence_changed(FOCUSED);
    output.wait_until("unfrozen, the worker counts on", |lines| {
        counts(lines) > frozen_at
    });
    end(&running);
}

/// How long a program stays hidden before the engine freezes it, in the freezing test.
const AFTER_HIDDEN: Duration = Duration::from_millis(300);

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

/// The workers program, running under an engine and a shell of its own, its output its own.
fn run_workers() -> (WaOS, RunningProgram, Arc<RecordedOutput>) {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let workers = engine.load(Path::new(waos_test_programs::WORKERS)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&workers, printing_to(&output)).unwrap();
    (engine, running, output)
}

/// Stops the program, and waits until it and its workers have ended.
fn end(running: &RunningProgram) {
    running.stop();
    within(running.ended());
    assert_eq!(
        running.stats().workers(),
        0,
        "no worker outlives its program"
    );
}

/// How many counts the counting workers printed among `lines`.
fn counts(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|line| line.starts_with("worker count "))
        .count()
}

/// The count and the wakes of `said`, `<count>, the main instance awake <wakes> times meanwhile`.
fn primes_and_wakes(said: &str) -> (u32, u32) {
    let (count, rest) = said.split_once(", the main instance awake ").unwrap();
    let wakes = rest.strip_suffix(" times meanwhile").unwrap();
    (count.parse().unwrap(), wakes.parse().unwrap())
}
