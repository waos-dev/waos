//! Every route beneath a folder gives the walk's verdicts on the same paths: what stays inside
//! opens, and `..`, an absolute path and a symlink leading out are refused, with the walk's error,
//! the verdicts `experiments/2026-10-23-wasi-open-speed` found for `wasmtime-wasi`'s walk; and a
//! path ending in `/` or `/.` asks for a folder whichever route serves it, through a symlink the
//! status does not follow among them (wasmtime#14387) and on a file (wasmtime#14368).

// WaOS's own, beside the copy: linted as the rest of WaOS.
#![warn(clippy::all)]

use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::symlink;
use std::path::Path;

use rustix::io::Errno;
use tempfile::TempDir;

use crate::files::beneath::{FollowSymlinks, Metadata, OpenOptions, walk};

/// A mount's folder, beside a file and a folder outside it.
struct Tree {
    root: TempDir,
}

/// What each route is to give a path: an open for reading, a status following a symlink the path
/// ends in, and one that does not.
struct Verdicts {
    path: String,
    open: &'static str,
    status: &'static str,
    status_of_link: &'static str,
}

impl Tree {
    fn new() -> Tree {
        let root = TempDir::new().unwrap();
        let mount = root.path().join("mount");
        fs::create_dir_all(mount.join("d1")).unwrap();
        fs::write(mount.join("d1/f.txt"), "inside").unwrap();
        fs::write(root.path().join("outside.txt"), "outside").unwrap();
        fs::create_dir(root.path().join("outside-dir")).unwrap();
        symlink("d1/f.txt", mount.join("link-in")).unwrap();
        symlink("../outside.txt", mount.join("link-out")).unwrap();
        symlink(root.path().join("outside.txt"), mount.join("link-abs")).unwrap();
        symlink("d1", mount.join("link-dir-in")).unwrap();
        symlink("../outside-dir", mount.join("link-dir-out")).unwrap();
        Tree { root }
    }

    fn mount(&self) -> fs::File {
        fs::File::open(self.root.path().join("mount")).unwrap()
    }

    /// Each path tried, and the walk's verdicts on it.
    fn paths(&self) -> Vec<Verdicts> {
        let absolute = self.root.path().join("outside.txt").display().to_string();
        let verdicts = |path: &str, open, status, status_of_link| Verdicts {
            path: path.to_owned(),
            open,
            status,
            status_of_link,
        };
        vec![
            verdicts("d1/f.txt", "inside", "file", "file"),
            verdicts("d1/../d1/f.txt", "inside", "file", "file"),
            verdicts("../outside.txt", "refused", "refused", "refused"),
            verdicts(&absolute, "refused", "refused", "refused"),
            verdicts("link-in", "inside", "file", "symlink"),
            verdicts("link-out", "refused", "refused", "symlink"),
            verdicts("link-abs", "refused", "refused", "symlink"),
            verdicts("link-dir-in", "folder", "folder", "symlink"),
            verdicts("d1/", "folder", "folder", "folder"),
            verdicts("d1/.", "folder", "folder", "folder"),
            verdicts("link-dir-in/", "folder", "folder", "folder"),
            verdicts("link-dir-in/.", "folder", "folder", "folder"),
            verdicts("link-dir-out/", "refused", "refused", "refused"),
            verdicts("link-out/", "refused", "refused", "refused"),
            verdicts("link-in/", "not a folder", "not a folder", "not a folder"),
            verdicts("d1/f.txt/", "not a folder", "not a folder", "not a folder"),
            verdicts("d1/f.txt/.", "not a folder", "not a folder", "not a folder"),
        ]
    }
}

/// What opening a path for reading gave: a file's contents, a folder, or why it failed.
fn opened(result: io::Result<fs::File>) -> String {
    match result {
        Ok(file) if file.metadata().unwrap().is_dir() => "folder".to_owned(),
        Ok(mut file) => {
            let mut contents = String::new();
            file.read_to_string(&mut contents).unwrap();
            contents
        }
        Err(error) => failed(&error),
    }
}

/// What a path's status gave: what it is of, or why it failed.
fn stated(result: io::Result<Metadata>) -> String {
    match result {
        Ok(metadata) if metadata.file_type().is_symlink() => "symlink".to_owned(),
        Ok(metadata) if metadata.is_dir() => "folder".to_owned(),
        Ok(metadata) if metadata.file_type().is_file() => "file".to_owned(),
        Ok(metadata) => format!("something else: {:?}", metadata.file_type()),
        Err(error) => failed(&error),
    }
}

/// Why a call failed: refused as the walk refuses an escape, refused as asking for a folder at
/// something that is not one, or otherwise.
fn failed(error: &io::Error) -> String {
    let as_the_walk_refuses =
        error.kind() == io::ErrorKind::PermissionDenied && error.raw_os_error().is_none();
    if as_the_walk_refuses {
        "refused".to_owned()
    } else if error.raw_os_error() == Some(Errno::NOTDIR.raw_os_error()) {
        "not a folder".to_owned()
    } else {
        format!("failed otherwise: {error}")
    }
}

fn read() -> OpenOptions {
    OpenOptions::new().read(true).clone()
}

#[test]
fn the_walk_opens_inside_refuses_every_escape_and_reads_a_paths_end() {
    let tree = Tree::new();
    for verdicts in tree.paths() {
        let path = Path::new(&verdicts.path);
        let mount = tree.mount();
        assert_eq!(
            opened(walk::open(&mount, path, &read())),
            verdicts.open,
            "open {}",
            verdicts.path
        );
        assert_eq!(
            stated(walk::stat(&mount, path, FollowSymlinks::Yes)),
            verdicts.status,
            "status {}",
            verdicts.path
        );
        assert_eq!(
            stated(walk::stat(&mount, path, FollowSymlinks::No)),
            verdicts.status_of_link,
            "status of a link {}",
            verdicts.path
        );
    }
}

#[test]
fn the_beneath_calls_give_the_walks_verdicts_by_whichever_route_serves_them() {
    use crate::files::beneath::{open, stat};
    let tree = Tree::new();
    for verdicts in tree.paths() {
        let path = Path::new(&verdicts.path);
        let mount = tree.mount();
        assert_eq!(
            opened(open(&mount, path, &read())),
            verdicts.open,
            "open {}",
            verdicts.path
        );
        assert_eq!(
            stated(stat(&mount, path, FollowSymlinks::Yes)),
            verdicts.status,
            "status {}",
            verdicts.path
        );
        assert_eq!(
            stated(stat(&mount, path, FollowSymlinks::No)),
            verdicts.status_of_link,
            "status of a link {}",
            verdicts.path
        );
    }
}

#[cfg(target_vendor = "apple")]
#[test]
fn apples_flags_give_the_walks_verdicts_where_the_system_knows_them() {
    use crate::files::beneath::unix::{open_fast, opens_beneath, stat_fast, stats_beneath};
    let tree = Tree::new();
    for verdicts in tree.paths() {
        let path = Path::new(&verdicts.path);
        let mount = tree.mount();
        if opens_beneath() {
            let by_flag = open_fast(&mount, path, &read()).transpose().unwrap();
            assert_eq!(opened(by_flag), verdicts.open, "open {}", verdicts.path);
        }
        if stats_beneath() {
            for (follow, verdict) in [
                (FollowSymlinks::Yes, verdicts.status),
                (FollowSymlinks::No, verdicts.status_of_link),
            ] {
                let by_flag = stat_fast(&mount, path, follow).transpose().unwrap();
                assert_eq!(stated(by_flag), verdict, "{follow:?} {}", verdicts.path);
            }
        }
    }
}

#[cfg(target_vendor = "apple")]
#[test]
fn a_file_states_alike_through_apples_flag_through_the_walk_and_through_its_handle() {
    use crate::files::beneath::unix::{stat_fast, stats_beneath};
    use crate::files::status::FileStat;
    let tree = Tree::new();
    let path = Path::new("d1/f.txt");
    let folder = FileStat::from(&tree.mount().metadata().unwrap());
    let walked = FileStat::from(&walk::stat(&tree.mount(), path, FollowSymlinks::Yes).unwrap());
    let opened = fs::File::open(tree.root.path().join("mount").join(path)).unwrap();
    let through_handle = FileStat::from(&opened.metadata().unwrap());
    assert_eq!(walked.identity, through_handle.identity);
    assert_ne!(
        walked.identity, folder.identity,
        "the folder is another file"
    );
    if stats_beneath() {
        let by_flag = stat_fast(&tree.mount(), path, FollowSymlinks::Yes)
            .unwrap()
            .unwrap();
        let by_flag = FileStat::from(&by_flag);
        assert_eq!(by_flag.identity, through_handle.identity);
        assert_eq!(by_flag.status.size, through_handle.status.size);
        assert_eq!(
            by_flag.status.data_modification_timestamp,
            through_handle.status.data_modification_timestamp
        );
    }
}
