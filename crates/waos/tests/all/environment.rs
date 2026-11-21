//! `examples/environment` under the test shell: a run reads the variables whoever ran it gave it,
//! as `waos run --env NAME=VALUE` gives them, beside WaOS's own `TMPDIR`, which wins over a
//! variable of that name (the design's decision 68).

use std::sync::Arc;

use waos::{ProgramEnd, ProgramEnvironment, ProgramOutput};

use crate::shell::{self, Lines};

#[test]
fn a_run_reads_the_variables_it_was_given_and_waos_s_own_win() {
    let (engine, _shell, _arrivals) = shell::engine();
    let program = shell::load(&engine, waos_test_programs::ENVIRONMENT);
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        variables: vec![
            ("RUST_LOG".to_owned(), "debug".to_owned()),
            ("TMPDIR".to_owned(), "/elsewhere".to_owned()),
        ],
        ..ProgramEnvironment::default()
    };
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    lines.wait_for("RUST_LOG=debug");
    assert!(matches!(
        shell::wait_for_end(&running),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(
        lines.all(),
        ["RUST_LOG=debug", "TMPDIR=/tmp"],
        "WaOS's own TMPDIR wins over a variable of that name, and nothing else is passed"
    );
}

#[test]
fn a_runs_arguments_start_with_the_programs_name() {
    let (engine, _shell, _arrivals) = shell::engine();
    let program = shell::load(&engine, waos_test_programs::ARGUMENTS).named("dev.example.args");
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        arguments: vec!["--port".to_owned(), "8080".to_owned()],
        ..ProgramEnvironment::default()
    };
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    assert!(matches!(
        shell::wait_for_end(&running),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(
        lines.all(),
        [
            "argument: dev.example.args",
            "argument: --port",
            "argument: 8080"
        ]
    );
}
