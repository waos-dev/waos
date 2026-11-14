//! On Windows a change of a folder is made on the folder's handle, with the name as given: a name
//! Win32 would read otherwise, ending in a dot, is made, listed and removed as it is, where a path
//! call would have made `kept` of `kept.`; a rename and a link reach the other folder by its handle.

// WaOS's own, beside the copy: linted as the rest of WaOS.
#![warn(clippy::all)]

use std::fs;
use std::path::Path;

use super::dir_calls as h;
use super::sys_common::io::tmpdir;
use crate::files::beneath as p;

/// The names in `folder`, sorted.
fn names_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_name_ending_in_a_dot_is_made_and_removed_as_it_is() {
    let tmpdir = tmpdir();
    let start = h::dir_of(&tmpdir);
    check!(p::create_dir(&start, Path::new("kept.")));
    assert_eq!(names_in(tmpdir.path()), ["kept."]);
    check!(p::remove_dir(&start, Path::new("kept.")));
    assert!(names_in(tmpdir.path()).is_empty());
}

#[test]
fn a_rename_and_a_link_reach_the_other_folder_by_its_handle() {
    let tmpdir = tmpdir();
    let start = h::dir_of(&tmpdir);
    check!(p::create_dir(&start, Path::new("from")));
    check!(p::create_dir(&start, Path::new("to")));
    check!(h::write(&start, "from/note.txt", b"a note"));
    let (from, to) = (
        check!(p::open_dir(&start, Path::new("from"))),
        check!(p::open_dir(&start, Path::new("to"))),
    );
    check!(p::hard_link(
        &from,
        Path::new("note.txt"),
        &to,
        Path::new("linked.txt")
    ));
    check!(p::rename(
        &from,
        Path::new("note.txt"),
        &to,
        Path::new("moved.txt")
    ));
    assert!(names_in(&tmpdir.path().join("from")).is_empty());
    assert_eq!(
        names_in(&tmpdir.path().join("to")),
        ["linked.txt", "moved.txt"]
    );
    assert_eq!(
        fs::read_to_string(tmpdir.path().join("to/moved.txt")).unwrap(),
        "a note"
    );
    error!(
        p::hard_link(&to, Path::new("moved.txt"), &to, Path::new("linked.txt")),
        183
    );
}
