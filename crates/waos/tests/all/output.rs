//! Where a program's output goes: to the `ProgramOutput` in its environment, as it writes it and
//! in order, or, with none, to the engine's log. The program is `examples/forever`, which says
//! its count every so often and asks nothing else.
//!
//! A listener of the program's output, through its trace, hears each write from when it listens;
//! one that listens to a program started held hears its first write.
//!
//! The tests that read the engine's log run forever under a name of their own, so the lines
//! logged under it are their own run's.

use std::path::Path;
use std::sync::Arc;

use futures::StreamExt;
use waos::{ProgramEnd, ProgramEnvironment, TraceKind, TraceStream, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

/// Forever's first two reports, in order.
const FIRST_REPORTS: [&str; 2] = ["count 1048576", "count 2097152"];

#[test]
fn a_chosen_output_gets_what_the_program_writes_in_order_and_the_log_nothing() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let name = "dev.test.output-to-a-chosen-output";
    let forever = engine
        .load(Path::new(waos_test_programs::FOREVER))
        .unwrap()
        .named(name);
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&forever, printing_to(&output)).unwrap();
    output.wait_for(FIRST_REPORTS[1]);
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
    assert_eq!(output.lines()[..2], FIRST_REPORTS);
    // TODO(tests-without-the-clock): the test shell has no reader of the engine's log, so this
    // reads the old shell's, which records every program's lines by name.
    let logged = crate::shell::output().lines_of(0, name);
    assert!(
        logged.is_empty(),
        "the log got what the chosen output was for: {logged:?}"
    );
}

#[test]
fn with_no_output_chosen_the_program_writes_to_the_log() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let name = "dev.test.output-to-the-log";
    let forever = engine
        .load(Path::new(waos_test_programs::FOREVER))
        .unwrap()
        .named(name);
    // TODO(tests-without-the-clock): the test shell has no reader of the engine's log, so this
    // reads the old shell's, which records every program's lines by name.
    let log = crate::shell::output();
    let running = engine.run(&forever, ProgramEnvironment::default()).unwrap();
    log.wait_for(0, name, |line| line == FIRST_REPORTS[1]);
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
    assert_eq!(log.lines_of(0, name)[..2], FIRST_REPORTS);
}

#[test]
fn a_listener_of_the_programs_output_hears_what_it_writes_from_then_on() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let forever = engine.load(Path::new(waos_test_programs::FOREVER)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&forever, printing_to(&output)).unwrap();
    // Writing, it is linked, and so can be listened to.
    output.wait_for(FIRST_REPORTS[0]);
    let listened = running
        .trace(TraceKind::Output)
        .expect("linked, so it can be listened to");
    let bytes = first_write(listened);
    assert!(
        String::from_utf8_lossy(&bytes).starts_with("count "),
        "{bytes:?}"
    );
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_program_started_held_writes_nothing_until_let_go_and_a_listener_hears_its_first_write() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let forever = engine.load(Path::new(waos_test_programs::FOREVER)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let (running, held) = engine.run_held(&forever, printing_to(&output)).unwrap();
    let held = within(async move {
        let mut held = held;
        held.linked().await;
        held
    });
    let listened = running
        .trace(TraceKind::Output)
        .expect("linked, so it can be listened to");
    drop(held);
    // Its first write heard by a listener from while it was held: it wrote nothing before.
    let bytes = first_write(listened);
    assert_eq!(
        String::from_utf8_lossy(&bytes),
        "count 1048576\n",
        "its first write"
    );
    assert_eq!(output.lines()[0], FIRST_REPORTS[0]);
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

/// The bytes of the first write `listened` hears, on the program's standard output, from its
/// first instance.
fn first_write(listened: TraceStream) -> Vec<u8> {
    let chunk = within(async move {
        let mut listened = listened;
        listened.next().await
    })
    .expect("a write is heard");
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["channel"], "stdout");
    assert_eq!(json["instance"], 0);
    serde_json::from_value(json["bytes"].clone()).unwrap()
}
