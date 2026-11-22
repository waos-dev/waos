//! `examples/files` with its own `/data` and `/cache` and the test's grants: it reads, writes,
//! lists and moves files in its own folder and a writable mount, sees only what is mounted in
//! the folders above its mounts, is refused every write to a read-only mount, every escape and
//! every change above its mounts, finds nothing outside them, and sees a mount the user adds
//! while it runs, then its removal, reading on through a file it opened before.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use futures::executor::block_on;
use tempfile::TempDir;
use waos::{Answer, ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput, Question};
use waos_capabilities::{
    ChangedFuture, DomainRule, FileSystemGrant, Mount, NetworkGrant, ProgramFolders,
};

use crate::shell;
use crate::shell::Lines;

/// What the program prints before it waits for `/later`, in order.
const BEFORE_LATER: [&str; 26] = [
    "data read: hello",
    "data stat: 5",
    "data list: hello.txt",
    "rename across mounts: done",
    "work append: done",
    "work read: hello world",
    "root list: cache, data, mnt, tmp",
    "mnt list: docs, work",
    "mnt is a folder: true",
    "docs read: read me",
    "docs link inside: read me",
    "docs write: ReadOnlyFilesystem",
    "docs open to write: ReadOnlyFilesystem",
    "docs remove: ReadOnlyFilesystem",
    "escape by ..: PermissionDenied",
    "escape above the root: PermissionDenied",
    "escape by a symlink: PermissionDenied",
    "escape by an absolute symlink: PermissionDenied",
    "outside read: NotFound",
    "outside stat: NotFound",
    "outside list: NotFound",
    "root write: ReadOnlyFilesystem",
    "root make a folder: ReadOnlyFilesystem",
    "data make again: AlreadyExists",
    "mnt remove: ResourceBusy",
    "later read: NotFound",
];

#[test]
fn a_program_uses_its_mounts_and_nothing_outside_them() {
    let machine = Machine::new();
    let (engine, _shell, _arrivals) = shell::engine();
    let program = shell::load(&engine, waos_test_programs::FILES);
    let grants = Arc::new(TestGrants::new(machine.mounts()));
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        folders: Some(machine.own_folders()),
        ..ProgramEnvironment::default()
    };
    let running = engine.run(&program, environment).unwrap();
    lines.wait_for(BEFORE_LATER[BEFORE_LATER.len() - 1]);
    assert_eq!(lines.all(), BEFORE_LATER);

    grants.set_mounts(machine.mounts_with_later());
    lines.wait_for("later read: hi");
    grants.set_mounts(machine.mounts());
    lines.wait_for("later gone: NotFound");
    lines.wait_for("later read through the kept file: hi");
    let end = block_on(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");

    machine.assert_changed_only_where_the_program_may();
}

/// The folders the program is given, and a file beside them it must not reach.
struct Machine {
    root: TempDir,
}

impl Machine {
    fn new() -> Machine {
        let root = TempDir::new().unwrap();
        let path = root.path();
        for folder in ["data", "cache", "docs", "work", "later"] {
            fs::create_dir(path.join(folder)).unwrap();
        }
        fs::write(path.join("outside.txt"), "outside").unwrap();
        fs::write(path.join("docs/readme.txt"), "read me").unwrap();
        symlink("readme.txt", path.join("docs/link-in")).unwrap();
        symlink("../outside.txt", path.join("docs/link-out")).unwrap();
        symlink(path.join("outside.txt"), path.join("docs/link-abs")).unwrap();
        fs::write(path.join("later/hello.txt"), "hi").unwrap();
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

    /// `docs` read-only and `work` writable, under `/mnt`.
    fn mounts(&self) -> FileSystemGrant {
        FileSystemGrant {
            mounts: vec![
                mount(&self.folder("docs"), "/mnt/docs", false),
                mount(&self.folder("work"), "/mnt/work", true),
            ],
        }
    }

    /// [`Machine::mounts`], and `later` at `/later`.
    fn mounts_with_later(&self) -> FileSystemGrant {
        let mut mounts = self.mounts();
        mounts
            .mounts
            .push(mount(&self.folder("later"), "/later", false));
        mounts
    }

    /// The program moved its note to `work` and appended to it there, and changed nothing else.
    fn assert_changed_only_where_the_program_may(&self) {
        let read = |path: &str| fs::read_to_string(self.root.path().join(path)).unwrap();
        assert_eq!(read("work/hello.txt"), "hello world");
        assert!(
            self.folder("data/notes")
                .read_dir()
                .unwrap()
                .next()
                .is_none()
        );
        assert_eq!(read("docs/readme.txt"), "read me");
        assert!(!self.folder("docs/new.txt").exists());
        assert_eq!(read("outside.txt"), "outside");
        assert!(!self.root.path().join("new.txt").exists());
    }
}

pub(crate) fn mount(host: &Path, at: &str, writable: bool) -> Mount {
    Mount {
        host: host.to_owned(),
        at: PathBuf::from(at),
        writable,
    }
}

/// Grants of the test's own: nothing granted but the mounts, which the test changes as the user
/// would, each change heard by whoever waits for one.
pub(crate) struct TestGrants {
    mounts: Mutex<FileSystemGrant>,
    waiting: Mutex<Vec<oneshot::Sender<()>>>,
}

impl TestGrants {
    pub(crate) fn new(mounts: FileSystemGrant) -> TestGrants {
        TestGrants {
            mounts: Mutex::new(mounts),
            waiting: Mutex::default(),
        }
    }

    /// The user mounts these folders, and only these.
    fn set_mounts(&self, mounts: FileSystemGrant) {
        *self.mounts.lock().unwrap() = mounts;
        for waiting in self.waiting.lock().unwrap().drain(..) {
            let _ = waiting.send(());
        }
    }
}

impl ProgramGrants for TestGrants {
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
        self.mounts.lock().unwrap().clone()
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_answer(&self, _question: Question, _answer: Answer) {}

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        Some(Answer::Denied)
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

    fn changed(&self) -> ChangedFuture {
        let (changed, heard) = oneshot::channel();
        self.waiting.lock().unwrap().push(changed);
        Box::pin(async move {
            let _ = heard.await;
        })
    }
}
