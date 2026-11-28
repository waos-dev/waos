//! `examples/file-dialogs` under the test shell, whose dialogs answer the files and the folder of
//! a folder the test makes: a file picked to read is mounted read-only at its own path, where the
//! program reads it and cannot write to it; one picked writable is written; a save's target,
//! which does not exist yet, is written, then replaced whole by a file renamed onto it from the
//! program's temporary folder, `/tmp`; and a folder picked is read. A dialog whose
//! answer its user set is answered with it, mounted as a pick, and shows nothing; and a set answer
//! that does not hold now fails the dialog, saying why, with nothing shown in its place.

use std::fs;
use std::future::pending;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use waos::{Answer, ProgramEnd, ProgramEnvironment, ProgramGrants, Question, WaOS};
use waos_capabilities::{
    ChangedFuture, DomainRule, FileDialog, FileSystemGrant, NetworkGrant, ProgramFolders,
};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, wait_until, within};

#[test]
fn picks_are_mounted_as_asked_and_a_save_is_replaced_safely_from_the_programs_cache() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let picks = fs::canonicalize(scratch.path()).unwrap().join("picks");
    let pictures = picks.join("pictures");
    fs::create_dir_all(&pictures).unwrap();
    for (name, text) in [
        ("read-me.txt", "a line to read"),
        ("change-me.txt", "a line"),
    ] {
        fs::write(picks.join(name), text).unwrap();
    }
    for name in ["one.png", "two.png"] {
        fs::write(pictures.join(name), "PNG").unwrap();
    }
    let dialogs = &shell.file_dialogs;
    for pick in ["read-me.txt", "change-me.txt", "saved.txt", "pictures"] {
        dialogs.answer_next(vec![picks.join(pick)]);
    }
    let own = own_folders(scratch.path());
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        folders: Some(own.clone()),
        ..printing_to(&output)
    };
    let program = engine
        .load(
            Path::new(waos_test_programs::FILE_DIALOGS),
            &waos_test_programs::code_of(waos_test_programs::FILE_DIALOGS),
        )
        .unwrap();
    let running = engine.run(&program, environment).unwrap();
    let end = within(running.ended());
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "{:#?}",
        output.lines()
    );

    for line in [
        "temporary folder: /tmp",
        "read: a line to read",
        "read file written to: no",
        "changed: yes",
        "saved: yes",
        "saved safely: yes",
        "folder entries: 2",
    ] {
        assert!(output.has(line), "no {line:?} in {:#?}", output.lines());
    }
    let read = |name: &str| fs::read_to_string(picks.join(name)).unwrap();
    assert_eq!(read("read-me.txt"), "a line to read");
    assert_eq!(read("change-me.txt"), "a line, changed");
    assert_eq!(read("saved.txt"), "second save", "replaced whole");
    assert_eq!(fs::read_dir(&own.cache).unwrap().count(), 0, "renamed away");
    let kinds: Vec<_> = dialogs.asked().iter().map(|asked| asked.kind).collect();
    assert_eq!(
        kinds,
        ["open-file", "open-file", "save-file", "open-folder"]
    );
    assert_eq!(
        dialogs.asked()[2].suggested_name.as_deref(),
        Some("saved.txt")
    );
}

#[test]
fn dialogs_whose_answers_are_set_are_answered_with_them_and_show_nothing() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let picks = fs::canonicalize(scratch.path()).unwrap().join("picks");
    fs::create_dir_all(picks.join("pictures")).unwrap();
    fs::write(picks.join("read-me.txt"), "a line to read").unwrap();
    fs::write(picks.join("pictures").join("one.png"), "PNG").unwrap();
    let grants = SetAnswers {
        open_file: picks.join("read-me.txt"),
        save_file: picks.join("saved.txt"),
        open_folder: picks.join("pictures"),
    };
    let (output, end) = run_file_dialogs(&engine, scratch.path(), grants);
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "{:#?}",
        output.lines()
    );
    for line in [
        "read: a line to read",
        "read file written to: no",
        "changed: yes",
        "saved: yes",
        "folder entries: 1",
    ] {
        assert!(output.has(line), "no {line:?} in {:#?}", output.lines());
    }
    assert_eq!(
        fs::read_to_string(picks.join("read-me.txt")).unwrap(),
        "a line to read, changed",
        "answered again to the open asked writable, mounted writable"
    );
    assert!(shell.file_dialogs.asked().is_empty(), "no dialog shown");
}

#[test]
fn a_set_answer_that_does_not_hold_fails_the_dialog_with_why_and_shows_nothing() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let missing = fs::canonicalize(scratch.path())
        .unwrap()
        .join("missing.txt");
    let grants = SetAnswers {
        open_file: missing.clone(),
        save_file: missing.join("saved.txt"),
        open_folder: missing.clone(),
    };
    let (output, _end) = run_file_dialogs(&engine, scratch.path(), grants);
    let said = format!(
        "open: failed (WaOS answers this dialog with {}, as the program's settings say, but it does not exist)",
        missing.display()
    );
    assert!(output.has(&said), "no {said:?} in {:#?}", output.lines());
    let folder_failed = output.lines().iter().any(|line| {
        line.starts_with("folder: failed") && line.ends_with("but it is not a folder)")
    });
    assert!(folder_failed, "{:#?}", output.lines());
    assert!(
        shell.file_dialogs.asked().is_empty(),
        "no dialog shown in its place"
    );
}

#[test]
fn a_waiting_file_dialog_is_answered_by_setting_its_path_and_drops_its_prompt() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let picks = fs::canonicalize(scratch.path()).unwrap().join("picks");
    fs::create_dir_all(picks.join("pictures")).unwrap();
    let read_me = picks.join("read-me.txt");
    fs::write(&read_me, "a line to read").unwrap();

    let dialogs = &shell.file_dialogs;
    dialogs.hold_next();
    let grants = Arc::new(LiveFileDialogGrants::default());
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        folders: Some(own_folders(scratch.path())),
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        ..printing_to(&output)
    };
    let program = engine
        .load(
            Path::new(waos_test_programs::FILE_DIALOGS),
            &waos_test_programs::code_of(waos_test_programs::FILE_DIALOGS),
        )
        .unwrap();
    let running = engine.run(&program, environment).unwrap();

    wait_until("the open-file dialog is asked", || {
        !dialogs.asked().is_empty()
    });
    assert!(!dialogs.held_cancelled(0));

    // Setting an unrelated dialog's path leaves the waiting open-file dialog up.
    let reads = grants.reads();
    grants.set(FileDialog::OpenFolder, picks.join("pictures"));
    wait_until("the open-file answer is read again", || {
        grants.reads() > reads
    });
    assert!(
        !dialogs.held_cancelled(0),
        "unrelated dialog setting left open-file waiting"
    );
    assert!(!output.has("read: a line to read"));

    // Setting open-file answers it and drops the shell's dialog future.
    grants.set(FileDialog::OpenFile, read_me);
    wait_until("the waiting dialog future is dropped", || {
        dialogs.held_cancelled(0)
    });
    output.wait_for("read: a line to read");
    running.stop();
    within(running.ended());
}

#[derive(Default)]
struct LiveFileDialogGrants {
    open_file: Mutex<Option<PathBuf>>,
    open_folder: Mutex<Option<PathBuf>>,
    reads: AtomicUsize,
    changed: Arc<Notify>,
}

impl LiveFileDialogGrants {
    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn set(&self, dialog: FileDialog, path: PathBuf) {
        match dialog {
            FileDialog::OpenFile => *self.open_file.lock().unwrap() = Some(path),
            FileDialog::OpenFolder => *self.open_folder.lock().unwrap() = Some(path),
            _ => {}
        }
        self.changed.notify_waiters();
    }
}

impl ProgramGrants for LiveFileDialogGrants {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: DomainRule::Only(Vec::new()),
        }
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant::default()
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn file_dialog_answer(&self, dialog: FileDialog) -> Option<Vec<PathBuf>> {
        if dialog == FileDialog::OpenFile {
            self.reads.fetch_add(1, Ordering::SeqCst);
        }
        match dialog {
            FileDialog::OpenFile => self.open_file.lock().unwrap().clone().map(|p| vec![p]),
            FileDialog::OpenFolder => self.open_folder.lock().unwrap().clone().map(|p| vec![p]),
            _ => None,
        }
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(Arc::clone(&self.changed).notified_owned())
    }
}

/// Runs `examples/file-dialogs` to its end with `grants`, its own folders beneath `scratch`.
fn run_file_dialogs(
    engine: &WaOS,
    scratch: &Path,
    grants: SetAnswers,
) -> (Arc<RecordedOutput>, ProgramEnd) {
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        folders: Some(own_folders(scratch)),
        grants: Some(Arc::new(grants) as Arc<dyn ProgramGrants>),
        ..printing_to(&output)
    };
    let program = engine
        .load(
            Path::new(waos_test_programs::FILE_DIALOGS),
            &waos_test_programs::code_of(waos_test_programs::FILE_DIALOGS),
        )
        .unwrap();
    let running = engine.run(&program, environment).unwrap();
    let end = within(running.ended());
    (output, end)
}

/// Grants that switch nothing on and answer each dialog with the path set for it.
struct SetAnswers {
    open_file: PathBuf,
    save_file: PathBuf,
    open_folder: PathBuf,
}

impl ProgramGrants for SetAnswers {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: DomainRule::Only(Vec::new()),
        }
    }

    fn local_network(&self) -> bool {
        false
    }

    fn clipboard_read(&self) -> bool {
        false
    }

    fn clipboard_write(&self) -> bool {
        false
    }

    fn file_system(&self) -> FileSystemGrant {
        FileSystemGrant::default()
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn file_dialog_answer(&self, dialog: FileDialog) -> Option<Vec<PathBuf>> {
        let path = match dialog {
            FileDialog::OpenFile | FileDialog::OpenFiles => &self.open_file,
            FileDialog::OpenFolder => &self.open_folder,
            FileDialog::SaveFile => &self.save_file,
        };
        Some(vec![path.clone()])
    }

    fn changed(&self) -> ChangedFuture {
        Box::pin(pending())
    }
}

/// The program's own `/data` and `/cache`, made beneath `scratch`.
fn own_folders(scratch: &Path) -> ProgramFolders {
    let own = ProgramFolders {
        data: scratch.join("data"),
        cache: scratch.join("cache"),
    };
    fs::create_dir_all(&own.data).unwrap();
    fs::create_dir_all(&own.cache).unwrap();
    own
}
