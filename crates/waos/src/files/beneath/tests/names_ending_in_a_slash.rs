//! A change of a folder named through a symlink with a `/` after it, `link/`, changes nothing the
//! symlink leads to, whether it leads out of the folder or stays inside: cap-std's rename followed
//! such a name out of the folder on Apple's systems (GHSA-m9f7-9w4p-gv38), and WasmEdge's rename,
//! folder making and removal and hard link did (WasmEdge 617d724). Each call either refuses or
//! fails, and the folder outside, the folder inside and the symlink stay as they were. On Windows
//! the symlinks are folder symlinks, which a user makes with Developer Mode on or the privilege to,
//! and the tests are skipped without it, as the copy's are.

// WaOS's own, beside the copy: linted as the rest of WaOS.
#![warn(clippy::all)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tempfile::TempDir;

use super::sys_common::symlink_supported;
use crate::files::beneath as p;

/// Makes a symlink to the folder `target` at `link`.
fn symlink_to_folder(target: &str, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

/// A symlink to the file `target` at `link` beneath `start`, as the copy makes one.
fn symlink_to_file(target: &Path, start: &fs::File, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    return p::symlink(target, start, link);
    #[cfg(windows)]
    return p::symlink_file(target, start, link);
}

/// A mount's folder, holding a folder, a file and a symlink to each folder, and a folder beside it
/// with a file in it.
struct Tree {
    root: TempDir,
}

impl Tree {
    fn new() -> Tree {
        let root = TempDir::new().unwrap();
        let mount = root.path().join("mount");
        fs::create_dir_all(mount.join("inside")).unwrap();
        fs::write(mount.join("inside/kept.txt"), "inside").unwrap();
        fs::create_dir(mount.join("folder")).unwrap();
        fs::write(mount.join("file.txt"), "a file").unwrap();
        fs::create_dir(root.path().join("outside")).unwrap();
        fs::write(root.path().join("outside/secret.txt"), "outside").unwrap();
        symlink_to_folder("../outside", &mount.join("link-out"));
        symlink_to_folder("inside", &mount.join("link-in"));
        Tree { root }
    }

    fn mount(&self) -> fs::File {
        p::open_ambient_dir(&self.root.path().join("mount")).unwrap()
    }

    /// What the folders hold, the symlinks and the times of what they lead to, which no call may
    /// change.
    fn state(&self) -> Vec<String> {
        let mount = self.root.path().join("mount");
        let mut state = Vec::new();
        for folder in [
            self.root.path().join("outside"),
            mount.join("inside"),
            mount,
        ] {
            let mut names: Vec<String> = fs::read_dir(&folder)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            let modified = fs::metadata(&folder).unwrap().modified().unwrap();
            state.push(format!("{}: {names:?}, {modified:?}", folder.display()));
        }
        for link in ["link-out", "link-in"] {
            let path = self.root.path().join("mount").join(link);
            state.push(format!("{link} -> {:?}", fs::read_link(path).unwrap()));
        }
        state
    }
}

/// A call that names `link/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    RenameFrom,
    RenameAFolderOnto,
    RenameAFileOnto,
    CreateDir,
    RemoveDir,
    RemoveFile,
    HardLinkFrom,
    HardLinkOnto,
    SymlinkOnto,
    SetTimes,
    SetTimesOfTheLink,
}

/// The calls that would replace or remove what `link/` leads to, or make something in its place.
const CHANGES_OF_A_FOLDER: [Call; 9] = [
    Call::RenameFrom,
    Call::RenameAFolderOnto,
    Call::RenameAFileOnto,
    Call::CreateDir,
    Call::RemoveDir,
    Call::RemoveFile,
    Call::HardLinkFrom,
    Call::HardLinkOnto,
    Call::SymlinkOnto,
];

/// Makes `call` on `link/` beneath `start`.
fn make(call: Call, start: &fs::File, link: &str) -> io::Result<()> {
    let named = PathBuf::from(format!("{link}/"));
    let an_hour_ago = Some(SystemTime::now() - Duration::from_secs(3600));
    match call {
        Call::RenameFrom => p::rename(start, &named, start, Path::new("moved")),
        Call::RenameAFolderOnto => p::rename(start, Path::new("folder"), start, &named),
        Call::RenameAFileOnto => p::rename(start, Path::new("file.txt"), start, &named),
        Call::CreateDir => p::create_dir(start, &named),
        Call::RemoveDir => p::remove_dir(start, &named),
        Call::RemoveFile => p::remove_file(start, &named),
        Call::HardLinkFrom => p::hard_link(start, &named, start, Path::new("linked")),
        Call::HardLinkOnto => p::hard_link(start, Path::new("file.txt"), start, &named),
        Call::SymlinkOnto => symlink_to_file(Path::new("file.txt"), start, &named),
        Call::SetTimes => p::set_times(start, &named, an_hour_ago, an_hour_ago),
        Call::SetTimesOfTheLink => p::set_times_nofollow(start, &named, an_hour_ago, an_hour_ago),
    }
}

#[test]
fn no_call_through_a_symlink_named_with_a_slash_reaches_out_of_the_folder() {
    if !symlink_supported() {
        return;
    }
    let calls = CHANGES_OF_A_FOLDER
        .into_iter()
        .chain([Call::SetTimes, Call::SetTimesOfTheLink]);
    for call in calls {
        let tree = Tree::new();
        let before = tree.state();
        let result = make(call, &tree.mount(), "link-out");
        // Windows opens a nofollow name as the link itself even with a `/` after it, where Unix
        // names the folder it leads to and refuses it out of the folder; the state below says
        // either way that nothing the link leads to changed.
        let names_the_link_alone = cfg!(windows) && call == Call::SetTimesOfTheLink;
        assert!(names_the_link_alone || result.is_err(), "{call:?} was made");
        assert_eq!(
            tree.state(),
            before,
            "{call:?} changed something: {result:?}"
        );
    }
}

#[test]
fn no_change_through_a_symlink_named_with_a_slash_replaces_or_removes_what_it_leads_to_inside() {
    if !symlink_supported() {
        return;
    }
    for call in CHANGES_OF_A_FOLDER {
        let tree = Tree::new();
        let before = tree.state();
        let result = make(call, &tree.mount(), "link-in");
        assert!(result.is_err(), "{call:?} was made");
        assert_eq!(
            tree.state(),
            before,
            "{call:?} changed something: {result:?}"
        );
    }
}

/// Unix's errors by their text; on Windows a trailing slash is read before the call, as the copy's
/// Windows tests have it.
#[cfg(unix)]
#[test]
fn a_folder_named_with_a_slash_is_renamed_and_a_file_is_not() {
    let tree = Tree::new();
    let start = tree.mount();
    check!(p::rename(
        &start,
        Path::new("folder/"),
        &start,
        Path::new("renamed")
    ));
    assert!(tree.root.path().join("mount/renamed").is_dir());
    error!(
        p::rename(
            &start,
            Path::new("file.txt/"),
            &start,
            Path::new("renamed.txt")
        ),
        "Not a directory"
    );
    assert!(tree.root.path().join("mount/file.txt").is_file());
}
