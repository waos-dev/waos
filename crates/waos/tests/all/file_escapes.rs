//! `examples/file-escapes` with its own `/data` beside a folder of the machine, and the test's
//! mounts, a writable one beside a read-only one: through `wasi:filesystem`'s calls on folders it
//! opened, it changes nothing a symlink named with a `/` after it leads to, out of `/data` or inside
//! it (GHSA-m9f7-9w4p-gv38, WasmEdge 617d724); reads a symlink to a folder named so as the folder
//! and a file named so as no folder (wasmtime#14387 and #14368); and reaches nothing of the
//! read-only mount from the writable one, by `..` or a symlink, nor renames into or out of it
//! (Wasmer's GHSA-6936-225w-643x, which checked a call's rights on the folder it was given rather
//! than the one it reached); and answers the same through `std::fs` from the root, whose paths keep
//! their `/` through the mount table.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::executor::block_on;
use tempfile::TempDir;
use waos::{ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput};
use waos_capabilities::{FileSystemGrant, ProgramFolders};

use crate::files::{TestGrants, mount};
use crate::shell;
use crate::shell::Lines;

/// What the program prints, in order: each escape refused, by `..` or a symlink out of its mount as
/// leaving it, `not-permitted`, and each call through a symlink named with a `/` after it as the
/// system answers it where nothing is followed out, `/` asking for a folder.
const PRINTED: [&str; 61] = [
    "make link-out: done",
    "make link-in: done",
    "make link-to-folder: done",
    "rename from link-out/: not-directory",
    "rename a folder onto link-out/: not-directory",
    "rename a file onto link-out/: not-directory",
    "create-directory link-out/: exist",
    "remove-directory link-out/: not-permitted",
    "unlink-file link-out/: not-permitted",
    "link from link-out/: not-permitted",
    "link onto link-out/: not-permitted",
    "symlink onto link-out/: not-permitted",
    "rename from link-in/: not-directory",
    "rename a folder onto link-in/: not-directory",
    "rename a file onto link-in/: not-directory",
    "create-directory link-in/: exist",
    "remove-directory link-in/: invalid",
    "unlink-file link-in/: not-permitted",
    "link from link-in/: not-permitted",
    "link onto link-in/: exist",
    "symlink onto link-in/: exist",
    "status link-to-folder: symlink",
    "status followed link-to-folder: folder",
    "status link-to-folder/: folder",
    "status followed link-to-folder/: folder",
    "status file.txt/: not-directory",
    "status followed file.txt/: not-directory",
    "status file.txt/.: not-directory",
    "status followed file.txt/.: not-directory",
    "make to-docs: done",
    "create ../docs/new.txt: not-permitted",
    "create-directory ../docs/made: not-permitted",
    "rename note.txt to ../docs/note.txt: not-permitted",
    "link note.txt to ../docs/linked: not-permitted",
    "unlink-file ../docs/readme.txt: not-permitted",
    "create to-docs/new.txt: not-permitted",
    "create-directory to-docs/made: not-permitted",
    "rename note.txt to to-docs/note.txt: not-permitted",
    "link note.txt to to-docs/linked: not-permitted",
    "unlink-file to-docs/readme.txt: not-permitted",
    "create docs-abs/new.txt: not-permitted",
    "create-directory docs-abs/made: not-permitted",
    "rename note.txt to docs-abs/note.txt: not-permitted",
    "link note.txt to docs-abs/linked: not-permitted",
    "unlink-file docs-abs/readme.txt: not-permitted",
    "rename note.txt into docs' folder: read-only",
    "rename readme.txt out of docs' folder: read-only",
    "std write /mnt/work/../docs/new.txt: PermissionDenied",
    "std write /mnt/work/to-docs/new.txt: PermissionDenied",
    "std status /data/file.txt/: NotADirectory",
    "std status /data/file.txt/.: NotADirectory",
    "std status /data/link-to-folder/: folder",
    "std rename from /data/link-out/: NotADirectory",
    "std create a folder /data/link-out/: AlreadyExists",
    "std remove the folder /data/link-out/: PermissionDenied",
    "std remove the file /data/link-out/: PermissionDenied",
    "std rename from /data/link-in/: NotADirectory",
    "std create a folder /data/link-in/: AlreadyExists",
    "std remove the folder /data/link-in/: InvalidInput",
    "std remove the file /data/link-in/: PermissionDenied",
    "done",
];

#[test]
fn a_program_leaves_its_mounts_by_no_name_ending_in_a_slash_and_by_no_mount_beside_another() {
    let machine = Machine::new();
    let (engine, _shell, _arrivals) = shell::engine();
    let program = shell::load(&engine, waos_test_programs::FILE_ESCAPES);
    let grants = Arc::new(TestGrants::new(machine.mounts()));
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        grants: Some(grants as Arc<dyn ProgramGrants>),
        folders: Some(machine.own_folders()),
        ..ProgramEnvironment::default()
    };
    let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
    let end = block_on(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert_eq!(lines.all(), PRINTED);
    machine.assert_unchanged_outside_data_and_in_docs();
}

/// The program's own folders, a folder beside `data` it must not reach, and the test's mounts:
/// `work`, holding a symlink to `docs` by its path on the machine, and `docs` beside it.
struct Machine {
    root: TempDir,
}

impl Machine {
    fn new() -> Machine {
        let root = TempDir::new().unwrap();
        let path = root.path();
        for folder in ["data", "cache", "outside", "work", "docs"] {
            fs::create_dir(path.join(folder)).unwrap();
        }
        fs::write(path.join("outside/secret.txt"), "outside").unwrap();
        fs::write(path.join("docs/readme.txt"), "read me").unwrap();
        symlink(path.join("docs"), path.join("work/docs-abs")).unwrap();
        Machine { root }
    }

    fn folder(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn own_folders(&self) -> ProgramFolders {
        ProgramFolders {
            data: self.folder("data"),
            cache: self.folder("cache"),
        }
    }

    /// `work` writable and `docs` read-only, beside each other under `/mnt`.
    fn mounts(&self) -> FileSystemGrant {
        FileSystemGrant {
            mounts: vec![
                mount(&self.folder("work"), "/mnt/work", true),
                mount(&self.folder("docs"), "/mnt/docs", false),
            ],
        }
    }

    /// The folder beside `data` and `docs` hold what they held, and `data` what the program made
    /// in it, its symlinks still symlinks.
    fn assert_unchanged_outside_data_and_in_docs(&self) {
        assert_eq!(self.names("outside"), ["secret.txt"]);
        assert_eq!(self.read("outside/secret.txt"), "outside");
        assert_eq!(self.names("docs"), ["readme.txt"]);
        assert_eq!(self.read("docs/readme.txt"), "read me");
        assert_eq!(self.names("data/inside"), ["kept.txt"]);
        assert_eq!(
            self.names("data"),
            [
                "file.txt",
                "folder",
                "inside",
                "link-in",
                "link-out",
                "link-to-folder"
            ]
        );
        for (link, leads_to) in [("link-out", "../outside"), ("link-in", "inside")] {
            let path = self.folder("data").join(link);
            assert_eq!(fs::read_link(path).unwrap(), Path::new(leads_to), "{link}");
        }
        assert_eq!(self.read("work/note.txt"), "a note");
    }

    /// The names in the folder `name` holds, in order.
    fn names(&self, name: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.folder(name))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.folder(name)).unwrap()
    }
}
