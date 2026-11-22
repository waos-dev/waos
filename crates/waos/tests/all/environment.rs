//! `examples/environment` under the test shell: a run reads the variables whoever ran it gave it,
//! as `waos run --env NAME=VALUE` gives them, beside WaOS's own `TMPDIR`, which wins over a
//! variable of that name (the design's decision 68).

use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, ProgramEnvironment, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[test]
fn a_run_reads_the_variables_it_was_given_and_waos_s_own_win() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::ENVIRONMENT))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        variables: vec![
            ("RUST_LOG".to_owned(), "debug".to_owned()),
            ("TMPDIR".to_owned(), "/elsewhere".to_owned()),
        ],
        ..printing_to(&output)
    };
    let running = engine.run(&program, environment).unwrap();
    assert!(matches!(
        within(running.ended()),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(
        output.lines(),
        ["RUST_LOG=debug", "TMPDIR=/tmp"],
        "WaOS's own TMPDIR wins over a variable of that name, and nothing else is passed"
    );
}

#[test]
fn a_runs_arguments_start_with_the_programs_name() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::ARGUMENTS))
        .unwrap()
        .named("dev.example.args");
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        arguments: vec!["--port".to_owned(), "8080".to_owned()],
        ..printing_to(&output)
    };
    let running = engine.run(&program, environment).unwrap();
    assert!(matches!(
        within(running.ended()),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(
        output.lines(),
        [
            "argument: dev.example.args",
            "argument: --port",
            "argument: 8080"
        ]
    );
}
