//! How a program ends: `stop` ends one that never returns, one built for another version of the
//! interface ends failed, told apart as incompatible, and one that traps ends failed, its
//! backtrace naming the program's file and line; and what `load` and `compile` keep in a code
//! folder. That a
//! program stopped gives every GPU object it held back is the engine's own test, which reads
//! wgpu-core's registries from inside the crate.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use waos::{IncompatibleProgram, ProgramEnd, ProgramEnvironment, WaOS, WaOSConfig};

use crate::test_shell::{self, RecordedOutput, TestShell, printing_to, within};

#[test]
fn stop_ends_a_program_that_never_returns() {
    let engine = engine_with(test_shell::config());
    let forever = engine
        .load(
            Path::new(waos_test_programs::FOREVER),
            &waos_test_programs::code_of(waos_test_programs::FOREVER),
        )
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&forever, printing_to(&output)).unwrap();
    // Its first count says it runs the loop it never leaves.
    output.wait_for("count 1048576");
    let still_running = running.ended();
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
    assert!(
        matches!(within(still_running), ProgramEnd::Stopped),
        "every ended resolves with the same end"
    );
}

#[test]
fn a_program_that_traps_ends_failed() {
    let engine = engine_with(test_shell::config());
    let trap = engine
        .load(
            Path::new(waos_test_programs::TRAP),
            &waos_test_programs::code_of(waos_test_programs::TRAP),
        )
        .unwrap();
    let running = engine.run(&trap, ProgramEnvironment::default()).unwrap();
    let ProgramEnd::Failed(error) = within(running.ended()) else {
        panic!("a trap is a failure");
    };
    let error = format!("{error:#}");
    assert!(error.contains("unreachable"), "the trap is named: {error}");
    // The backtrace names the file as the machine that built it does, `\` between names on Windows.
    assert!(
        error
            .replace('\\', "/")
            .contains("/examples/trap/src/main.rs:"),
        "the backtrace names the program's file and line, from its line tables: {error}"
    );
}

#[test]
fn a_program_built_for_another_version_of_the_interface_ends_incompatible() {
    let engine = engine_with(test_shell::config());
    let incompatible = engine
        .load(
            Path::new(waos_test_programs::INCOMPATIBLE),
            &waos_test_programs::code_of(waos_test_programs::INCOMPATIBLE),
        )
        .unwrap();
    let running = engine
        .run(&incompatible, ProgramEnvironment::default())
        .unwrap();
    let ProgramEnd::Failed(error) = within(running.ended()) else {
        panic!("a program that cannot be linked fails");
    };
    let incompatible = error
        .downcast_ref::<IncompatibleProgram>()
        .unwrap_or_else(|| panic!("the failure is told apart: {error:#}"));
    assert!(
        incompatible
            .detail()
            .contains("wasdk:platform/system@1.0.0"),
        "wasmtime's words name the import: {}",
        incompatible.detail()
    );
}

#[test]
fn a_program_built_against_an_earlier_minor_version_of_the_interface_is_linked() {
    let engine = engine_with(test_shell::config());
    let earlier = engine
        .load(
            Path::new(waos_test_programs::BUILT_AGAINST_2_0),
            &waos_test_programs::code_of(waos_test_programs::BUILT_AGAINST_2_0),
        )
        .unwrap();
    let running = engine.run(&earlier, ProgramEnvironment::default()).unwrap();
    let ProgramEnd::Failed(error) = within(running.ended()) else {
        panic!("a program with no `wasi:cli/run` fails once linked");
    };
    assert!(
        error.downcast_ref::<IncompatibleProgram>().is_none(),
        "`wasdk:platform/system@2.0.0` links against the 2.2.0 the engine serves: {error:#}"
    );
}

#[test]
fn a_trap_is_not_incompatible() {
    let engine = engine_with(test_shell::config());
    let trap = engine
        .load(
            Path::new(waos_test_programs::TRAP),
            &waos_test_programs::code_of(waos_test_programs::TRAP),
        )
        .unwrap();
    let running = engine.run(&trap, ProgramEnvironment::default()).unwrap();
    let ProgramEnd::Failed(error) = within(running.ended()) else {
        panic!("a trap is a failure");
    };
    assert!(error.downcast_ref::<IncompatibleProgram>().is_none());
}

#[test]
fn a_program_loaded_twice_is_compiled_once() {
    let code = tempfile::tempdir().unwrap();
    let engine = engine_with(config(false));
    let trap = Path::new(waos_test_programs::TRAP);
    engine.load(trap, code.path()).unwrap();
    let compiled = compiled_files(code.path());
    assert_eq!(compiled.len(), 1, "one file per engine: {compiled:?}");
    let written = file_identity(&compiled[0]);
    engine.load(trap, code.path()).unwrap();
    assert_eq!(
        file_identity(&compiled[0]),
        written,
        "the second load mapped the file"
    );
}

#[test]
fn a_program_compiled_ahead_is_loaded_without_compiling() {
    let code = tempfile::tempdir().unwrap();
    let engine = engine_with(config(false));
    let trap = Path::new(waos_test_programs::TRAP);
    assert!(!engine.is_compiled(code.path()), "nothing compiled yet");
    engine.compile(trap, code.path()).unwrap();
    assert!(engine.is_compiled(code.path()));
    let compiled = compiled_files(code.path());
    assert_eq!(compiled.len(), 1, "one file per engine: {compiled:?}");
    let written = file_identity(&compiled[0]);
    engine.load(trap, code.path()).unwrap();
    assert_eq!(
        file_identity(&compiled[0]),
        written,
        "the load mapped the file compiled ahead"
    );
}

/// wasmtime maps code compiled with native unwind information into an engine without it, and the
/// other way round, so a code folder keeps the two apart; the engine compiling last removes the
/// other's code.
#[test]
fn code_compiled_with_native_unwind_information_is_never_mapped_without_it() {
    let code = tempfile::tempdir().unwrap();
    let trap = Path::new(waos_test_programs::TRAP);
    let with = engine_with(config(true));
    with.load(trap, code.path()).unwrap();
    let without = engine_with(config(false));
    // Windows keeps native unwind information whatever the engine asks: one setting, one file.
    assert_eq!(without.is_compiled(code.path()), cfg!(windows));
    without.load(trap, code.path()).unwrap();
    assert_eq!(
        compiled_files(code.path()).len(),
        1,
        "the other engine's code went"
    );
    assert_eq!(with.is_compiled(code.path()), cfg!(windows));
}

/// An engine set up as `config` says, over a test shell.
fn engine_with(config: WaOSConfig) -> WaOS {
    let shell = TestShell::new();
    WaOS::new(config, shell.shell(), shell.platform()).unwrap()
}

/// The tests' engine, compiling with native unwind information or without it.
fn config(native_unwind_info: bool) -> WaOSConfig {
    WaOSConfig {
        native_unwind_info,
        ..test_shell::config()
    }
}

/// Which file is at `path`: a file compiled again is a new one, renamed over the one before.
#[cfg(unix)]
fn file_identity(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).unwrap();
    (metadata.dev(), metadata.ino())
}

#[cfg(windows)]
fn file_identity(path: &Path) -> (u64, u64) {
    use std::os::windows::fs::MetadataExt;
    let metadata = fs::metadata(path).unwrap();
    (
        u64::from(metadata.volume_serial_number().unwrap()),
        metadata.file_index().unwrap(),
    )
}

/// The code files in `code`, a code folder.
fn compiled_files(code: &Path) -> Vec<PathBuf> {
    fs::read_dir(code)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "cwasm")
        })
        .collect()
}
