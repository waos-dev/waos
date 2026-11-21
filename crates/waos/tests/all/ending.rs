//! How a program ends: `stop` ends one that never returns, one built for another version of the
//! interface ends failed, told apart as incompatible, and one that traps ends failed, its
//! backtrace naming the program's file and line; and what `load`, `compile` and
//! `remove_compiled` keep in the code cache. That a
//! program stopped gives every GPU object it held back is the engine's own test, which reads
//! wgpu-core's registries from inside the crate.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use futures::executor::block_on;
use waos::{IncompatibleProgram, ProgramEnd, ProgramEnvironment, RunningProgram, WaOSConfig};

use crate::shell;

#[test]
fn stop_ends_a_program_that_never_returns() {
    // Forever logs its count, which other tests read in the log.
    let _forever = shell::turns::take("forever");
    let (engine, _shell, _arrivals) = shell::engine();
    let forever = shell::load(&engine, waos_test_programs::FOREVER);
    let running = engine.run(&forever, ProgramEnvironment::default()).unwrap();
    wait_for_fuel_read(&running);
    let still_running = running.ended();
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
    assert!(
        matches!(block_on(still_running), ProgramEnd::Stopped),
        "every ended resolves with the same end"
    );
}

#[test]
fn a_program_that_traps_ends_failed() {
    let (engine, _shell, _arrivals) = shell::engine();
    let trap = shell::load(&engine, waos_test_programs::TRAP);
    let running = engine.run(&trap, ProgramEnvironment::default()).unwrap();
    let ProgramEnd::Failed(error) = block_on(running.ended()) else {
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
    let (engine, _shell, _arrivals) = shell::engine();
    let incompatible = shell::load(&engine, waos_test_programs::INCOMPATIBLE);
    let running = engine
        .run(&incompatible, ProgramEnvironment::default())
        .unwrap();
    let ProgramEnd::Failed(error) = block_on(running.ended()) else {
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
fn a_trap_is_not_incompatible() {
    let (engine, _shell, _arrivals) = shell::engine();
    let trap = shell::load(&engine, waos_test_programs::TRAP);
    let running = engine.run(&trap, ProgramEnvironment::default()).unwrap();
    let ProgramEnd::Failed(error) = block_on(running.ended()) else {
        panic!("a trap is a failure");
    };
    assert!(error.downcast_ref::<IncompatibleProgram>().is_none());
}

#[test]
fn a_program_loaded_twice_is_compiled_once() {
    let cache = CodeCache::empty("loaded-twice");
    let (engine, _shell, _arrivals) = shell::engine_with(cache.config(false));
    shell::load(&engine, waos_test_programs::TRAP);
    let compiled = compiled_files(&cache.0, "trap");
    assert_eq!(compiled.len(), 1, "one file per program: {compiled:?}");
    let modified = fs::metadata(&compiled[0]).unwrap().modified().unwrap();
    shell::load(&engine, waos_test_programs::TRAP);
    assert_eq!(
        compiled_files(&cache.0, "trap"),
        compiled,
        "the second load mapped the file"
    );
    assert_eq!(
        modified,
        fs::metadata(waos_test_programs::TRAP)
            .unwrap()
            .modified()
            .unwrap(),
        "the file is dated as the component"
    );
}

#[test]
fn a_program_compiled_ahead_is_compiled_once_and_loaded_without_compiling() {
    let cache = CodeCache::empty("compiled-ahead");
    let (engine, _shell, _arrivals) = shell::engine_with(cache.config(false));
    let trap = Path::new(waos_test_programs::TRAP);
    assert!(!engine.is_compiled(trap).unwrap(), "nothing compiled yet");
    engine.compile(trap).unwrap();
    assert!(engine.is_compiled(trap).unwrap());
    let compiled = compiled_files(&cache.0, "trap");
    assert_eq!(compiled.len(), 1, "one file per program: {compiled:?}");
    let written = file_identity(&compiled[0]);
    engine.compile(trap).unwrap();
    assert_eq!(
        file_identity(&compiled[0]),
        written,
        "a second compile does nothing"
    );
    shell::load(&engine, waos_test_programs::TRAP);
    assert_eq!(
        file_identity(&compiled[0]),
        written,
        "the load mapped the file compiled ahead"
    );
}

#[test]
fn removing_a_programs_code_leaves_none_of_it_and_the_next_load_compiles() {
    let cache = CodeCache::empty("removed-code");
    let (engine, _shell, _arrivals) = shell::engine_with(cache.config(false));
    let trap = Path::new(waos_test_programs::TRAP);
    let loaded = shell::load(&engine, waos_test_programs::TRAP);
    engine.remove_compiled(trap).unwrap();
    assert_eq!(live_files(&cache.0, "trap"), Vec::<PathBuf>::new());
    assert!(!engine.is_compiled(trap).unwrap());
    engine.remove_compiled(trap).unwrap();
    let running = engine.run(&loaded, ProgramEnvironment::default()).unwrap();
    assert!(
        matches!(block_on(running.ended()), ProgramEnd::Failed(_)),
        "a program loaded before keeps the code it mapped, and runs to its trap"
    );
    drop((running, loaded));
    shell::load(&engine, waos_test_programs::TRAP);
    assert_eq!(
        compiled_files(&cache.0, "trap").len(),
        1,
        "the code the program mapped went once nothing mapped it"
    );
}

/// wasmtime maps code compiled with native unwind information into an engine without it, and the
/// other way round, so the cache keeps the two apart.
#[test]
fn code_compiled_with_native_unwind_information_is_never_mapped_without_it() {
    let cache = CodeCache::empty("unwind-information");
    let (with, _shell, _arrivals) = shell::engine_with(cache.config(true));
    shell::load(&with, waos_test_programs::TRAP);
    let (without, _shell, _arrivals) = shell::engine_with(cache.config(false));
    shell::load(&without, waos_test_programs::TRAP);
    let compiled = compiled_files(&cache.0, "trap");
    // Windows keeps native unwind information whatever the engine asks: one setting, one file.
    let settings = if cfg!(windows) { 1 } else { 2 };
    assert_eq!(
        compiled.len(),
        settings,
        "one file per program and setting: {compiled:?}"
    );
}

/// Waits until an epoch tick has read the fuel `program` burned, failing after [`shell::WITHIN`].
fn wait_for_fuel_read(program: &RunningProgram) {
    let deadline = Instant::now() + shell::WITHIN;
    while program.stats().fuel_consumed() == 0 {
        assert!(Instant::now() < deadline, "no tick read the fuel");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A code cache of one test's own, empty when made, and removed with what it holds when dropped.
struct CodeCache(PathBuf);

impl CodeCache {
    fn empty(test: &str) -> CodeCache {
        let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("code-cache-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        CodeCache(folder)
    }

    /// The tests' engine set up with this cache, compiling with native unwind information or
    /// without it.
    fn config(&self, native_unwind_info: bool) -> WaOSConfig {
        WaOSConfig {
            code_cache: self.0.clone(),
            native_unwind_info,
            ..shell::config()
        }
    }
}

impl Drop for CodeCache {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Which file is at `path`: a file compiled again is a new one, renamed over the one before,
/// though it carries the same modification time.
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

/// The compiled files of the program `name` in `cache`.
/// The files of `name`'s code in `cache` that a load may map: on Windows the code cache keeps a
/// file a program maps after its removal until nothing does, dated at the epoch, never mapped.
fn live_files(cache: &Path, name: &str) -> Vec<PathBuf> {
    compiled_files(cache, name)
        .into_iter()
        .filter(|file| fs::metadata(file).unwrap().modified().unwrap() != SystemTime::UNIX_EPOCH)
        .collect()
}

fn compiled_files(cache: &Path, name: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(cache) else {
        return Vec::new();
    };
    entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let file = path.file_name().unwrap().to_string_lossy();
            file.starts_with(&format!("{name}-")) && file.ends_with(".cwasm")
        })
        .collect()
}
