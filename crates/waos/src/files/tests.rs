//! A program's path calls as its access tells them: an open and a change decided by the mounts,
//! refused by `folders` outside them and where a mount is read-only; a status only noted; a
//! handle's first write noted once; and a file opened kept among the open files while it is held.

use std::fs;
use std::sync::Arc;

use tempfile::TempDir;
use waos_capabilities::ProgramFolders;

use super::*;
use crate::access::Told;
use crate::permissions::NothingGranted;

/// A program's files: its own `/data`, and a file the user pasted, read-only, at the path the
/// program names it by from its root, which this answers.
struct Files {
    _scratch: TempDir,
    files: ProgramFiles,
    access: ProgramAccess,
    pasted: String,
}

fn files() -> Files {
    let scratch = tempfile::tempdir().unwrap();
    let (data, cache) = (scratch.path().join("data"), scratch.path().join("cache"));
    fs::create_dir(&data).unwrap();
    fs::create_dir(&cache).unwrap();
    let pasted = fs::canonicalize(scratch.path()).unwrap().join("pasted.txt");
    fs::write(&pasted, "pasted").unwrap();
    let handed = HandedFiles::default();
    handed.hand(&pasted);
    let access = ProgramAccess::new(Arc::new(NothingGranted));
    let folders = ProgramFolders { data, cache };
    let files = ProgramFiles::new(Some(&folders), None, access.clone(), handed);
    let pasted = machine_paths::in_program(&pasted).unwrap();
    Files {
        _scratch: scratch,
        files,
        access,
        pasted: pasted.to_string_lossy().trim_start_matches('/').to_owned(),
    }
}

fn reading() -> OpenRequest {
    OpenRequest {
        follow: true,
        read: true,
        ..OpenRequest::default()
    }
}

fn writing() -> OpenRequest {
    OpenRequest {
        follow: true,
        create: true,
        write: true,
        ..OpenRequest::default()
    }
}

#[test]
fn an_open_outside_the_mounts_or_writing_a_read_only_one_is_told_refused_by_the_folders() {
    let Files {
        _scratch,
        mut files,
        access,
        pasted,
    } = files();
    let root = ProgramFiles::root();
    assert!(matches!(
        files.open(&root, "elsewhere/secret.txt", &reading()),
        Err(ErrorCode::NoEntry)
    ));
    assert!(matches!(
        files.open(&root, &pasted, &writing()),
        Err(ErrorCode::ReadOnly)
    ));
    assert!(matches!(
        files.change(&root, &pasted, FileChange::Remove),
        Err(ErrorCode::ReadOnly)
    ));
    let to_outside = FileChange::Rename { to: "elsewhere/x" };
    assert!(matches!(
        files.change_between(&root, &pasted, &root, to_outside),
        Err(ErrorCode::ReadOnly)
    ));
    assert_eq!(access.refusals(), ["folders"; 4]);
    assert!(
        files.open(&root, &pasted, &reading()).is_ok(),
        "read as pasted"
    );
}

#[test]
fn an_open_is_told_where_it_landed_and_held_open_while_its_handle_is() {
    let Files {
        _scratch,
        mut files,
        access,
        ..
    } = files();
    let opened = files
        .open(&ProgramFiles::root(), "data/note.txt", &writing())
        .unwrap();
    let Some(Told::Allowed(asked, resolved)) = access.told().last().cloned() else {
        panic!("the open is told allowed");
    };
    assert!(asked.contains("data/note.txt"), "{asked}");
    assert!(
        resolved.contains("OwnData") && resolved.contains("note.txt"),
        "{resolved}"
    );
    let open_files = access.holdings().open_files;
    assert_eq!(open_files.len(), 1);
    assert!(
        open_files[0]
            .path
            .as_ref()
            .unwrap()
            .ends_with("data/note.txt"),
        "named where the system says it is"
    );
    drop(opened);
    assert!(access.holdings().open_files.is_empty());
}

#[test]
fn a_status_is_noted_not_decided_and_a_handle_is_written_once() {
    let Files {
        _scratch,
        mut files,
        access,
        ..
    } = files();
    let root = ProgramFiles::root();
    assert!(files.named(&root, "elsewhere").is_ok());
    assert!(matches!(access.told()[..], [Told::Noted(_)]), "no refusal");

    let Descriptor::File(opened) = files.open(&root, "data/log.txt", &writing()).unwrap() else {
        panic!("a file");
    };
    files.writing(&opened);
    files.writing(&opened.clone());
    let first_writes = access
        .told()
        .into_iter()
        .filter(|told| matches!(told, Told::Noted(noted) if noted.starts_with("FirstWrite")))
        .count();
    assert_eq!(first_writes, 1, "once per handle, whichever copy writes");
}
