//! `examples/file-dialogs` under the test shell, whose dialogs answer the files and the folder of
//! a folder the test makes: a file picked to read is mounted read-only at its own path, where the
//! program reads it and cannot write to it; one picked writable is written; a save's target,
//! which does not exist yet, is written, then replaced whole by a file renamed onto it from the
//! program's temporary folder, which is in `/cache`; and a folder picked is read. A dialog whose
//! answer its user set is answered with it, mounted as a pick, and shows nothing; and a set answer
//! that does not hold now fails the dialog, saying why, with nothing shown in its place.

use std::fs;
use std::future::pending;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use waos::{Answer, ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput, Question};
use waos_capabilities::{
    ChangedFuture, DomainRule, FileDialog, FileSystemGrant, NetworkGrant, ProgramFolders,
};

use crate::shell;
use crate::shell::Lines;

#[test]
fn picks_are_mounted_as_asked_and_a_save_is_replaced_safely_from_the_programs_cache() {
    let (engine, shell, _arrivals) = shell::engine();
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
    let dialogs = shell.file_dialogs();
    for pick in ["read-me.txt", "change-me.txt", "saved.txt", "pictures"] {
        dialogs.answer_next(vec![picks.join(pick)]);
    }
    let own = own_folders(scratch.path());
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        folders: Some(own.clone()),
        ..ProgramEnvironment::default()
    };
    let program = shell::load(&engine, waos_test_programs::FILE_DIALOGS);
    let (running, _given) = engine.run(&program, environment, Vec::new()).unwrap();
    let end = shell::wait_for_end(&running);
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "{:#?}",
        lines.all()
    );

    for line in [
        "temporary folder: /cache",
        "read: a line to read",
        "read file written to: no",
        "changed: yes",
        "saved: yes",
        "saved safely: yes",
        "folder entries: 2",
    ] {
        assert!(lines.has(line), "no {line:?} in {:#?}", lines.all());
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
    let (engine, shell, _arrivals) = shell::engine();
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
    let (lines, end) = run_file_dialogs(&engine, scratch.path(), grants);
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "{:#?}",
        lines.all()
    );
    for line in [
        "read: a line to read",
        "read file written to: no",
        "changed: yes",
        "saved: yes",
        "folder entries: 1",
    ] {
        assert!(lines.has(line), "no {line:?} in {:#?}", lines.all());
    }
    assert_eq!(
        fs::read_to_string(picks.join("read-me.txt")).unwrap(),
        "a line to read, changed",
        "answered again to the open asked writable, mounted writable"
    );
    assert!(shell.file_dialogs().asked().is_empty(), "no dialog shown");
}

#[test]
fn a_set_answer_that_does_not_hold_fails_the_dialog_with_why_and_shows_nothing() {
    let (engine, shell, _arrivals) = shell::engine();
    let scratch = tempfile::tempdir().unwrap();
    let missing = fs::canonicalize(scratch.path())
        .unwrap()
        .join("missing.txt");
    let grants = SetAnswers {
        open_file: missing.clone(),
        save_file: missing.join("saved.txt"),
        open_folder: missing.clone(),
    };
    let (lines, _end) = run_file_dialogs(&engine, scratch.path(), grants);
    let said = format!(
        "open: failed (WaOS answers this dialog with {}, as the program's settings say, but it does not exist)",
        missing.display()
    );
    assert!(lines.has(&said), "no {said:?} in {:#?}", lines.all());
    let folder_failed = lines.all().iter().any(|line| {
        line.starts_with("folder: failed") && line.ends_with("but it is not a folder)")
    });
    assert!(folder_failed, "{:#?}", lines.all());
    assert!(
        shell.file_dialogs().asked().is_empty(),
        "no dialog shown in its place"
    );
}

/// Runs `examples/file-dialogs` to its end with `grants`, its own folders beneath `scratch`.
fn run_file_dialogs(
    engine: &waos::WaOS,
    scratch: &Path,
    grants: SetAnswers,
) -> (Arc<Lines>, ProgramEnd) {
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        folders: Some(own_folders(scratch)),
        grants: Some(Arc::new(grants) as Arc<dyn ProgramGrants>),
        ..ProgramEnvironment::default()
    };
    let program = shell::load(engine, waos_test_programs::FILE_DIALOGS);
    let (running, _given) = engine.run(&program, environment, Vec::new()).unwrap();
    let end = shell::wait_for_end(&running);
    (lines, end)
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

    fn set_answer(&self, _question: Question, _answer: Answer) {}

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

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
