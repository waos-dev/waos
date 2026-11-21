//! Whether a program has a UI, read from its component alone: one that imports
//! `wasdk:platform/views` has one, as every program built on a framework's adapter does, and one
//! that does not is a console program, whatever else of wasdk it uses.

use crate::shell;

#[test]
fn a_program_that_never_reaches_for_a_view_is_a_console_program() {
    let (engine, _shell, _arrivals) = shell::engine();
    for console in [waos_test_programs::FOREVER, waos_test_programs::ENVIRONMENT] {
        assert!(
            shell::load(&engine, console).is_console_program(),
            "{console}"
        );
    }
}

#[test]
fn a_program_that_asks_for_a_view_or_draws_through_an_adapter_has_a_ui() {
    let (engine, _shell, _arrivals) = shell::engine();
    for drawn in [
        waos_test_programs::FOREVER_SHOWN,
        waos_test_programs::SURVEY,
        waos_test_programs::NOTES,
        waos_test_programs::TRAP,
    ] {
        assert!(!shell::load(&engine, drawn).is_console_program(), "{drawn}");
    }
}
