//! `examples/file-watching`, with no folders of its own and no grants: its `/tmp`, which `TMPDIR`
//! names, takes a file; a watch of a folder in it tells a file made, written, renamed and removed
//! by its path in the folder, the rename one rename where the system pairs its ends and each end a
//! `moved` where it does not, as FSEvents, and a file beneath only where the watch is recursive;
//! neither the root nor a file can be watched. And a program's `/tmp`, a folder of its own in the
//! engine's folder of them, goes when it ends, whether it returned, trapped or was stopped.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, WaOS, WaOSConfig};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

/// How the program's rename comes: paired by inotify and Windows, each end a `moved` by FSEvents.
const RENAMED: &str = if cfg!(target_os = "macos") {
    "renamed: note.txt to kept.txt, each end"
} else {
    "renamed: note.txt to kept.txt, paired"
};

/// What the program says, in order.
const SAID: [&str; 11] = [
    "TMPDIR: /tmp",
    "tmp holds: temporary",
    "created: note.txt",
    "modified: note.txt",
    RENAMED,
    "removed: kept.txt",
    "recursive: sub/deep.txt and marker.txt",
    "own entries: marker.txt alone",
    "watch the root: unsupported",
    "watch a file: not-directory",
    "done",
];

#[test]
fn a_program_watches_a_folder_in_its_tmp_and_its_tmp_goes_when_it_returns() {
    let folders = tempfile::tempdir().unwrap();
    let shell = TestShell::new();
    let engine = engine_with_temporary_folders(&shell, folders.path());
    let program = engine
        .load(
            Path::new(waos_test_programs::FILE_WATCHING),
            &waos_test_programs::code_of(waos_test_programs::FILE_WATCHING),
        )
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&program, printing_to(&output)).unwrap();
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert_eq!(output.lines(), SAID);
    assert_eq!(
        left_in(folders.path()),
        Vec::<String>::new(),
        "its /tmp went"
    );
}

#[test]
fn a_programs_tmp_goes_when_it_traps_and_when_it_is_stopped() {
    let folders = tempfile::tempdir().unwrap();
    let shell = TestShell::new();
    let engine = engine_with_temporary_folders(&shell, folders.path());
    let output = Arc::new(RecordedOutput::default());

    let trap = engine
        .load(
            Path::new(waos_test_programs::TRAP),
            &waos_test_programs::code_of(waos_test_programs::TRAP),
        )
        .unwrap();
    let running = engine.run(&trap, printing_to(&output)).unwrap();
    assert!(matches!(within(running.ended()), ProgramEnd::Failed(_)));
    assert_eq!(
        left_in(folders.path()),
        Vec::<String>::new(),
        "after a trap"
    );

    let forever = engine
        .load(
            Path::new(waos_test_programs::FOREVER),
            &waos_test_programs::code_of(waos_test_programs::FOREVER),
        )
        .unwrap();
    let running = engine.run(&forever, printing_to(&output)).unwrap();
    let held = left_in(folders.path());
    assert_eq!(held.len(), 2, "its folder and the lock beside it: {held:?}");
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
    assert_eq!(
        left_in(folders.path()),
        Vec::<String>::new(),
        "after a stop"
    );
}

/// An engine over `shell`, its programs' `/tmp` made in `folders`.
fn engine_with_temporary_folders(shell: &TestShell, folders: &Path) -> WaOS {
    let config = WaOSConfig {
        temporary_folders: folders.to_owned(),
        ..config()
    };
    WaOS::new(config, shell.shell(), shell.platform()).unwrap()
}

/// What is in `folders`, by name.
fn left_in(folders: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folders)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
