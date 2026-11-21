//! `examples/workers` under the test shell, each test reading the program's own output: a worker
//! at an entry the program does not have is refused as unknown; three words cross to a worker and
//! back, in order; a worker counts primes while the main instance goes on waking; a worker that
//! traps ends failed; a worker starts workers of its own; a worker asking for the user's side gets
//! none of it; a worker dropped mid-work ends; a program that ends, or is stopped, with a worker
//! running is told ended once the worker has ended too; and a frozen program's worker stops
//! counting until the thaw.
//!
//! The program runs its steps in order and prints each, and ends by starting a counting worker
//! and waiting to be asked to exit; a test reads the step it is about, then ends it.

use std::sync::Arc;
use std::time::Duration;

use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::{
    FreezePolicy, ManualTime, Presence, ProgramEnd, ProgramEnvironment, RunningProgram, WaOS,
};

use crate::shell::{self, Lines, resolved, wait_until, wait_until_stopped_at_freeze};

/// The primes below 200,000, which the program counts on a worker.
const PRIMES_BELOW_200_000: u32 = 17_984;

/// What the program's last step says once its counting worker runs and it waits to be asked to
/// exit.
const COUNTING: &str = "counting: until asked to exit";

#[test]
fn an_entry_the_program_does_not_have_is_refused_as_unknown() {
    let (_engine, running, lines) = run_workers();
    lines.wait_for("unknown entry: nothing");
    end(&running);
}

#[test]
fn messages_cross_to_a_worker_and_back_in_order() {
    let (_engine, running, lines) = run_workers();
    lines.wait_for("reversed: eno owt eerht");
    lines.wait_for("reverse finished: ok");
    end(&running);
}

#[test]
fn a_worker_computes_while_the_main_instance_goes_on() {
    let (_engine, running, lines) = run_workers();
    let prefix = "primes below 200000: ";
    wait_until("the primes are counted", || {
        lines.all().iter().any(|line| line.starts_with(prefix))
    });
    let line = lines
        .all()
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
    let (_engine, running, lines) = run_workers();
    wait_until("the trap is told", || {
        lines
            .all()
            .iter()
            .any(|line| line.starts_with("trap finished: failed"))
    });
    end(&running);
}

#[test]
fn a_worker_starts_workers_of_its_own() {
    let (_engine, running, lines) = run_workers();
    lines.wait_for("relayed: relay");
    lines.wait_for("relay finished: ok");
    end(&running);
}

#[test]
fn a_worker_has_none_of_the_users_side() {
    let (_engine, running, lines) = run_workers();
    lines.wait_for(
        "user's side in a worker: views 0, request unsupported, compositor none, clipboard none, \
         menu bar none, file dialogs none, notifications none, opener none, audio none, media \
         players none, cameras none, exit requests end",
    );
    end(&running);
}

#[test]
fn a_worker_dropped_mid_work_ends() {
    let (_engine, running, lines) = run_workers();
    lines.wait_for("dropped: a counting worker");
    lines.wait_for(COUNTING);
    wait_until("only the last step's worker runs", || {
        running.stats().workers() == 1
    });
    end(&running);
}

#[test]
fn a_program_that_ends_with_a_worker_running_is_told_ended_once_the_worker_has() {
    let (_engine, running, lines) = run_workers();
    lines.wait_for(COUNTING);
    assert_eq!(
        resolved(running.request_exit()),
        AppExitResponse::Exit,
        "the program answers exit"
    );
    let end = resolved(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    lines.wait_for("exit: with a worker counting");
    assert_eq!(running.stats().workers(), 0);
}

#[test]
fn a_program_stopped_with_a_worker_running_is_told_stopped_once_the_worker_has_ended() {
    let (_engine, running, lines) = run_workers();
    lines.wait_for(COUNTING);
    running.stop();
    assert!(matches!(resolved(running.ended()), ProgramEnd::Stopped));
    assert_eq!(running.stats().workers(), 0);
}

#[test]
fn a_frozen_programs_worker_stops_counting_until_the_thaw() {
    let time = ManualTime::new();
    let (engine, shell, _arrivals) = shell::engine_on(
        &time,
        FreezePolicy {
            after_hidden: Some(AFTER_HIDDEN),
        },
    );
    let lines = Arc::new(Lines::default());
    let (running, view) = shell::run_on_reserved(
        &engine,
        &shell,
        &shell::load(&engine, waos_test_programs::WORKERS),
        environment(&lines),
    );
    lines.wait_for(COUNTING);
    let first_count = counts(&lines).len();
    wait_until("the worker counts", || counts(&lines).len() > first_count);

    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN);
    wait_until_stopped_at_freeze(&running);
    let frozen_at = counts(&lines).len();
    time.elapse(AFTER_HIDDEN);
    wait_until_stopped_at_freeze(&running);
    assert_eq!(
        counts(&lines).len(),
        frozen_at,
        "frozen, the worker counts nothing"
    );

    view.presence_changed(FOCUSED);
    wait_until("thawed, the worker counts on", || {
        counts(&lines).len() > frozen_at
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

/// The workers program, running under an engine of its own with no view, its output its own.
fn run_workers() -> (Arc<WaOS>, RunningProgram, Arc<Lines>) {
    let (engine, _shell, _arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let running = engine
        .run(
            &shell::load(&engine, waos_test_programs::WORKERS),
            environment(&lines),
        )
        .unwrap();
    (engine, running, lines)
}

fn environment(lines: &Arc<Lines>) -> ProgramEnvironment {
    ProgramEnvironment {
        output: Some(Arc::clone(lines) as _),
        ..ProgramEnvironment::default()
    }
}

/// Stops the program, and waits until it and its workers have ended.
fn end(running: &RunningProgram) {
    running.stop();
    resolved(running.ended());
    assert_eq!(
        running.stats().workers(),
        0,
        "no worker outlives its program"
    );
}

/// The counts a counting worker printed.
fn counts(lines: &Lines) -> Vec<String> {
    lines
        .all()
        .into_iter()
        .filter(|line| line.starts_with("worker count "))
        .collect()
}

/// The count and the wakes of `said`, `<count>, the main instance awake <wakes> times meanwhile`.
fn primes_and_wakes(said: &str) -> (u32, u32) {
    let (count, rest) = said.split_once(", the main instance awake ").unwrap();
    let wakes = rest.strip_suffix(" times meanwhile").unwrap();
    (count.parse().unwrap(), wakes.parse().unwrap())
}
