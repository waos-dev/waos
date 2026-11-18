//! A program's path calls as its access tells them: an open and a change decided by the mounts,
//! refused by `folders` outside them and where a mount is read-only; a status only noted; a
//! handle's first write noted once; and a file opened kept among the open files while it is held.

use std::fs;
use std::path::Path;
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
    let open_files = access.open_files();
    assert_eq!(open_files.len(), 1);
    assert!(
        open_files[0].0.as_ref().unwrap().ends_with("data/note.txt"),
        "named where the system says it is"
    );
    drop(opened);
    assert!(access.open_files().is_empty());
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

#[test]
fn a_path_handed_to_the_system_is_decided_as_an_open_on_the_programs_behalf() {
    let Files {
        _scratch,
        mut files,
        access,
        pasted,
    } = files();
    assert!(
        files
            .machine_path(Path::new("/elsewhere/secret.txt"), Cause::Opener)
            .is_none()
    );
    assert_eq!(access.refusals(), ["folders"]);
    let pasted = files.machine_path(Path::new(&format!("/{pasted}")), Cause::Clipboard);
    assert!(pasted.is_some_and(|path| path.ends_with("pasted.txt")));
    let Some(Told::Allowed(asked, _)) = access.told().last().cloned() else {
        panic!("the open on the program's behalf is told allowed");
    };
    assert!(asked.contains("Clipboard"), "{asked}");
}

// Windows' root of this computer is the folder of its drives, which no handle holds.
#[cfg(unix)]
#[test]
fn a_machines_root_is_among_the_open_files_while_the_program_holds_it() {
    let access = ProgramAccess::new(Arc::new(NothingGranted));
    let root = folder_of_machine(Path::new("/"), &access).unwrap();
    let open_files = access.open_files();
    assert_eq!(open_files.len(), 1);
    assert_eq!(open_files[0].1, MountKind::Machine);
    drop(root);
    assert!(access.open_files().is_empty());
}

/// Notes' save, as `builtin-apps/notes/src/safe_write.rs` makes it through `std`: its folder made
/// where it is not, a draft written beside the note, then renamed onto it.
fn save_as_notes_saves(files: &mut ProgramFiles, contents: &str) {
    let root = ProgramFiles::root();
    let made = files.change(&root, "data/notes", FileChange::CreateDirectory);
    match made.and_then(|target| target.create_directory()) {
        Ok(()) | Err(ErrorCode::Exist) => {}
        Err(code) => panic!("the folder is made: {code:?}"),
    }
    let folder = files
        .named(&root, "data/notes")
        .unwrap()
        .status(true)
        .unwrap();
    assert_eq!(folder.type_, status::DescriptorType::Directory);
    let Descriptor::File(draft) = files
        .open(&root, "data/notes/5.json.saving", &writing())
        .expect("the draft opens")
    else {
        panic!("the draft is a file");
    };
    std::io::Write::write_all(&mut &**draft.handle(), contents.as_bytes()).unwrap();
    // Windows renames no file another handle holds open, and the engine opens every path
    // sharing reading and writing alone, as wasmtime-wasi does; Unix renames it either way.
    #[cfg(windows)]
    drop(draft);
    let rename = FileChange::Rename {
        to: "data/notes/5.json",
    };
    let (from, to) = files
        .change_between(&root, "data/notes/5.json.saving", &root, rename)
        .expect("the rename is allowed");
    from.rename_to(&to)
        .expect("the draft is renamed onto the note");
}

#[test]
fn a_note_is_saved_as_notes_saves_it_by_every_route() {
    let Files {
        _scratch,
        mut files,
        ..
    } = files();
    save_as_notes_saves(&mut files, "by the system's one call");
    beneath::walking(|| save_as_notes_saves(&mut files, "by the walk"));
    let Descriptor::File(note) = files
        .open(&ProgramFiles::root(), "data/notes/5.json", &reading())
        .unwrap()
    else {
        panic!("the note is a file");
    };
    let mut read = String::new();
    std::io::Read::read_to_string(&mut &**note.handle(), &mut read).unwrap();
    assert_eq!(read, "by the walk");
}
