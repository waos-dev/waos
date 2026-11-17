//! `examples/file-watching`, with no folders of its own and no grants: its `/tmp`, which `TMPDIR`
//! names, takes a file; a watch of a folder in it tells a file made, written, renamed and removed
//! by its path in the folder, the rename one rename where the system pairs its ends and each end a
//! `moved` where it does not, as FSEvents, and a file beneath only where the watch is recursive;
//! neither the root nor a file can be watched. And a program's `/tmp`, a folder of its own in the
//! engine's folder of them, goes when it ends, whether it returned, trapped or was stopped.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use futures::executor::block_on;
use waos::{ProgramEnd, ProgramEnvironment, ProgramOutput, WaOS, WaOSConfig};

use crate::shell;
use crate::shell::{Lines, TestShell};

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
    let (engine, _shell) = engine_with_temporary_folders(folders.path());
    let program = shell::load(&engine, waos_test_programs::FILE_WATCHING);
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    for line in SAID {
        lines.wait_for(line);
    }
    let end = block_on(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert_eq!(lines.all(), SAID);
    assert_eq!(
        left_in(folders.path()),
        Vec::<String>::new(),
        "its /tmp went"
    );
}

#[test]
fn a_programs_tmp_goes_when_it_traps_and_when_it_is_stopped() {
    let folders = tempfile::tempdir().unwrap();
    let (engine, _shell) = engine_with_temporary_folders(folders.path());

    let trap = shell::load(&engine, waos_test_programs::TRAP);
    let (running, _views) = engine
        .run(&trap, ProgramEnvironment::default(), Vec::new())
        .unwrap();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Failed(_)));
    assert_eq!(
        left_in(folders.path()),
        Vec::<String>::new(),
        "after a trap"
    );

    // Forever logs its count, which other tests read in the log.
    let _forever = shell::turns::take("forever");
    let forever = shell::load(&engine, waos_test_programs::FOREVER);
    let (running, _views) = engine
        .run(&forever, ProgramEnvironment::default(), Vec::new())
        .unwrap();
    let held = left_in(folders.path());
    assert_eq!(held.len(), 2, "its folder and the lock beside it: {held:?}");
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
    assert_eq!(
        left_in(folders.path()),
        Vec::<String>::new(),
        "after a stop"
    );
}

/// The tests' engine and its shell, its programs' `/tmp` made in `folders`.
fn engine_with_temporary_folders(folders: &Path) -> (Arc<WaOS>, Arc<TestShell>) {
    let (engine, shell, _arrivals) = shell::engine_with(WaOSConfig {
        temporary_folders: folders.to_owned(),
        audit: None,
        inspected_programs: None,
        ..shell::config()
    });
    (engine, shell)
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
