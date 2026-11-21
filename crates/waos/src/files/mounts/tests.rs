use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use tempfile::TempDir;
use waos_capabilities::{
    Answer, ChangedFuture, DomainRule, FileSystemGrant, Mount, NetworkGrant, ProgramFolders,
    ProgramGrants, Question,
};

use super::{Handed, HandedFiles, LiveMounts, MountKind, MountTable, Mounted, open_folder};
use crate::access::ProgramAccess;
use crate::files::errors::ErrorCode;
use crate::files::status::FileStat;
use crate::files::target::Target;
use crate::files::{Picks, machine_paths};
use crate::permissions::NothingGranted;

#[test]
fn a_path_leads_beneath_the_longest_mount_containing_it() {
    let machine = Machine::new(&["a", "c"]);
    let table = machine.table(&[("a", "/a"), ("c", "/a/b/c")]);
    assert_eq!(machine.leads(&table, "a/b/c/x"), "beneath c: x");
    assert_eq!(machine.leads(&table, "a/b/x"), "beneath a: b/x");
    assert_eq!(machine.leads(&table, "a/b"), "beneath a: b");
    assert_eq!(machine.leads(&table, "a/b/c"), "the mount c");
    assert_eq!(machine.leads(&table, "a"), "the mount a");
}

#[test]
fn names_above_the_mounts_are_taken_by_name_and_the_rest_left_to_the_system() {
    let machine = Machine::new(&["data", "cache", "docs"]);
    let table = machine.table(&[
        ("data", "/data"),
        ("cache", "/cache"),
        ("docs", "/mnt/docs"),
    ]);
    assert_eq!(machine.leads(&table, "mnt/../data/x"), "beneath data: x");
    assert_eq!(machine.leads(&table, "./mnt/./docs/x"), "beneath docs: x");
    assert_eq!(
        machine.leads(&table, "data/../cache"),
        "beneath data: ../cache",
        "`..` out of a mount is the system's to refuse"
    );
    assert_eq!(
        machine.leads_from(&table, "/mnt", ".."),
        "the folder above /"
    );
    assert_eq!(
        machine.leads_from(&table, "/mnt", "docs/x"),
        "beneath docs: x"
    );
}

#[test]
fn leaving_the_root_is_refused_as_wasmtime_wasi_refuses_leaving_a_preopened_folder() {
    let machine = Machine::new(&["data", "docs"]);
    let table = machine.table(&[("data", "/data"), ("docs", "/mnt/docs")]);
    for path in ["..", "/data/x", "mnt/../../data", "./.."] {
        assert_eq!(
            table.resolve(Path::new("/"), Path::new(path)).err(),
            Some(ErrorCode::NotPermitted),
            "{path}"
        );
    }
}

#[test]
fn a_path_outside_every_mount_is_not_found() {
    let machine = Machine::new(&["docs"]);
    let table = machine.table(&[("docs", "/mnt/docs")]);
    assert_eq!(machine.leads(&table, "etc"), "outside, in a folder above");
    assert_eq!(
        machine.leads(&table, "mnt/nothing"),
        "outside, in a folder above"
    );
    assert_eq!(machine.leads(&table, "etc/passwd"), "outside");
    assert_eq!(machine.leads(&table, "etc/../mnt"), "outside");
    assert_eq!(
        table.resolve(Path::new("/"), Path::new("")).err(),
        Some(ErrorCode::NoEntry),
        "an empty path names nothing"
    );
}

#[test]
fn a_paths_end_goes_beneath_the_mount_with_the_rest_of_it() {
    let machine = Machine::new(&["data", "docs"]);
    let note = machine.file("docs", "note.txt");
    let handed = HandedFiles::default();
    handed.hand(&note);
    let table = MountTable::new(&[], &machine.grant(&[("data", "/data")]), &handed.now());
    assert_eq!(
        machine.leads(&table, "data/file.txt/"),
        "beneath data: file.txt/"
    );
    assert_eq!(
        machine.leads(&table, "data/file.txt/."),
        "beneath data: file.txt/."
    );
    assert_eq!(machine.leads(&table, "data/link//"), "beneath data: link//");
    assert_eq!(
        machine.leads(&table, "data/x/../y/"),
        "beneath data: x/../y/"
    );
    assert_eq!(machine.leads(&table, "data/"), "the mount data");
    assert_eq!(machine.leads(&table, "data/."), "the mount data");
    let note = from_root(&note);
    let note = note.to_str().unwrap();
    assert_eq!(
        machine.leads(&table, &format!("{note}/")),
        "beneath docs: note.txt/",
        "a file mounted, asked for as a folder"
    );
    assert_eq!(machine.leads(&table, "./"), "the folder above /");
}

#[test]
fn a_folder_above_the_mounts_lists_only_what_is_mounted_under_it() {
    let machine = Machine::new(&["data", "cache", "docs", "work"]);
    let table = machine.table(&[
        ("data", "/data"),
        ("cache", "/cache"),
        ("docs", "/mnt/docs"),
        ("work", "/mnt/work"),
    ]);
    assert_eq!(table.names_under(Path::new("/")), ["cache", "data", "mnt"]);
    assert_eq!(table.names_under(Path::new("/mnt")), ["docs", "work"]);
    assert_eq!(machine.leads(&table, "mnt"), "the folder above /mnt");
    assert_eq!(machine.leads(&table, "."), "the folder above /");
}

#[test]
fn the_programs_own_folder_is_kept_over_a_grant_at_its_path() {
    let machine = Machine::new(&["own", "granted"]);
    let own = vec![
        Mounted::open(
            &machine.folder("own"),
            Path::new("/data"),
            true,
            MountKind::OwnData,
        )
        .unwrap(),
    ];
    let granted = machine.grant(&[("granted", "/data")]);
    let table = MountTable::new(&own, &granted, &Handed::default());
    assert_eq!(machine.leads(&table, "data/x"), "beneath own: x");
}

#[test]
fn a_mount_whose_folder_does_not_open_or_whose_path_is_not_of_names_is_left_out() {
    let machine = Machine::new(&["data"]);
    let granted = FileSystemGrant {
        mounts: vec![
            mount(&machine.root.path().join("missing"), "/missing"),
            mount(&machine.folder("data"), "relative"),
            mount(&machine.folder("data"), "/up/../data"),
        ],
    };
    let table = MountTable::new(&[], &granted, &Handed::default());
    assert!(table.names_under(Path::new("/")).is_empty());
}

#[test]
fn a_file_handed_is_mounted_read_only_at_its_own_path_and_nothing_else_of_its_folder_is() {
    let machine = Machine::new(&["pasted"]);
    let note = machine.file("pasted", "note.txt");
    machine.file("pasted", "other.txt");
    let handed = HandedFiles::default();
    handed.hand(&note);
    handed.hand(&note);
    let mut mounts = LiveMounts::new(
        None,
        None,
        ProgramAccess::new(Arc::new(TestGrants::default())),
        handed,
    );
    let table = mounts.now();
    let Target::Beneath(beneath) = table.resolve(Path::new("/"), &from_root(&note)).unwrap() else {
        panic!("the file is beneath its folder");
    };
    assert_eq!(machine.name_of(&beneath.folder), "pasted");
    assert_eq!(beneath.path, Path::new("note.txt"));
    assert!(!beneath.mount.writable);
    let other = note.with_file_name("other.txt");
    assert_eq!(
        machine.leads(table, from_root(&other).to_str().unwrap()),
        "outside, in a folder above"
    );
}

#[test]
fn a_file_handed_that_a_mount_reaches_at_its_path_keeps_that_mounts_access() {
    let machine = Machine::new(&["granted"]);
    let note = machine.file("granted", "note.txt");
    let granted = machine.grant_at_own_paths(&["granted"]);
    let handed = HandedFiles::default();
    handed.hand(&note);
    let table = MountTable::new(&[], &granted, &handed.now());
    let Target::Beneath(beneath) = table.resolve(Path::new("/"), &from_root(&note)).unwrap() else {
        panic!("the file is beneath the folder granted");
    };
    assert_eq!(beneath.path, Path::new("note.txt"));
    assert!(beneath.mount.writable, "as the folder granted is");
}

#[test]
fn an_inspector_reads_a_folders_entries_and_never_outside_its_mount() {
    let machine = Machine::new(&["data"]);
    machine.file("data", "note.txt");
    fs::create_dir(machine.folder("data").join("sub")).unwrap();
    let table = machine.table(&[("data", "/data")]);

    let entries = table.read_folder("/data").unwrap();
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["sub", "note.txt"], "folders first, by name");
    assert!(entries[0].is_folder);
    assert_eq!(entries[0].size, None, "a folder has no size");
    assert!(!entries[1].is_folder);
    assert_eq!(entries[1].size, Some(6));
    assert!(entries[1].modified.is_some());

    let above = table.read_folder("/").unwrap();
    assert!(
        above
            .iter()
            .any(|entry| entry.name == "data" && entry.is_folder)
    );

    assert_eq!(
        table.read_folder("/data/../..").err(),
        Some(ErrorCode::NotPermitted),
        "a path out of its mount"
    );
    assert_eq!(table.read_folder("data").err(), Some(ErrorCode::Invalid));
}

#[test]
fn a_mount_added_or_removed_applies_at_the_next_call() {
    let machine = Machine::new(&["data", "later"]);
    let grants = Arc::new(TestGrants::default());
    let folders = ProgramFolders {
        data: machine.folder("data"),
        cache: machine.root.path().join("no cache"),
    };
    let mut mounts = LiveMounts::new(
        Some(&folders),
        None,
        ProgramAccess::new(grants.clone()),
        HandedFiles::default(),
    );
    assert_eq!(
        machine.leads(mounts.now(), "later"),
        "outside, in a folder above"
    );
    assert_eq!(machine.leads(mounts.now(), "data"), "the mount data");

    grants.mount(machine.grant(&[("later", "/later")]));
    assert_eq!(machine.leads(mounts.now(), "later"), "the mount later");

    grants.mount(FileSystemGrant::default());
    assert_eq!(
        machine.leads(mounts.now(), "later"),
        "outside, in a folder above"
    );
}

#[test]
fn a_pick_the_grants_keep_is_mounted_once_and_unmounted_when_the_user_takes_it_back() {
    let machine = Machine::new(&["picked"]);
    let grants = Arc::new(TestGrants::default());
    let (picks, mut mounts) = picks_of(grants.clone());
    let picked = picks.mount(&machine.folder("picked"), true).unwrap();
    let at_pick = |table: &MountTable| {
        (table.mounts().into_iter())
            .filter(|mount| mount.at == Path::new(&picked))
            .count()
    };
    assert_eq!(at_pick(mounts.now()), 1);
    assert_eq!(
        machine.leads(mounts.now(), picked.trim_start_matches('/')),
        "the mount picked"
    );

    grants.mount(FileSystemGrant::default());
    assert_eq!(
        at_pick(mounts.now()),
        0,
        "taken back while the program runs"
    );
}

#[test]
fn a_file_picked_to_be_made_is_mounted_where_the_grants_keep_it() {
    let machine = Machine::new(&["docs"]);
    let (picks, mut mounts) = picks_of(Arc::new(TestGrants::default()));
    let target = machine.folder("docs").join("new.txt");
    let picked = picks.mount(&target, true).unwrap();
    assert_eq!(
        machine.leads(mounts.now(), picked.trim_start_matches('/')),
        "beneath docs: new.txt"
    );
}

#[test]
fn a_pick_is_mounted_for_the_run_where_the_grants_keep_nothing() {
    let machine = Machine::new(&["picked"]);
    let (picks, mut mounts) = picks_of(Arc::new(NothingGranted));
    let picked = picks.mount(&machine.folder("picked"), false).unwrap();
    let table = mounts.now();
    assert_eq!(
        machine.leads(table, picked.trim_start_matches('/')),
        "the mount picked"
    );
    let kinds: Vec<MountKind> = (table.mounts().into_iter())
        .filter(|mount| mount.at == Path::new(&picked))
        .map(|mount| mount.kind)
        .collect();
    assert_eq!(kinds, [MountKind::Picked]);
}

/// Where a program with `grants` has its picks mounted, and its mounts.
fn picks_of(grants: Arc<dyn ProgramGrants>) -> (Picks, LiveMounts) {
    let access = ProgramAccess::new(grants);
    let handed = HandedFiles::default();
    let mounts = LiveMounts::new(None, None, access.clone(), handed.clone());
    (Picks::new(handed, access), mounts)
}

/// Folders on the machine, each under a name the tests tell them apart by.
struct Machine {
    root: TempDir,
}

impl Machine {
    fn new(names: &[&str]) -> Machine {
        let root = TempDir::new().unwrap();
        for name in names {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        Machine { root }
    }

    fn folder(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    /// Each folder named, mounted writable where the program sees it.
    fn grant(&self, mounts: &[(&str, &str)]) -> FileSystemGrant {
        FileSystemGrant {
            mounts: mounts
                .iter()
                .map(|(name, at)| mount(&self.folder(name), at))
                .collect(),
        }
    }

    /// A file named `name` in the folder `folder`, made with a line in it.
    fn file(&self, folder: &str, name: &str) -> PathBuf {
        let path = self.folder(folder).join(name);
        fs::write(&path, "a line").unwrap();
        path
    }

    /// Each folder named, mounted writable at its own path, as a folder a user picks is.
    fn grant_at_own_paths(&self, names: &[&str]) -> FileSystemGrant {
        FileSystemGrant {
            mounts: names
                .iter()
                .map(|name| {
                    let folder = self.folder(name);
                    let own = machine_paths::in_program(&folder).unwrap();
                    mount(&folder, own.to_str().unwrap())
                })
                .collect(),
        }
    }

    fn table(&self, mounts: &[(&str, &str)]) -> MountTable {
        MountTable::new(&[], &self.grant(mounts), &Handed::default())
    }

    fn leads(&self, table: &MountTable, path: &str) -> String {
        self.leads_from(table, "/", path)
    }

    /// Where `path` leads from `from`, in words: a mount by its folder's name here.
    fn leads_from(&self, table: &MountTable, from: &str, path: &str) -> String {
        match table.resolve(Path::new(from), Path::new(path)).unwrap() {
            Target::Beneath(beneath) => format!(
                "beneath {}: {}",
                self.name_of(&beneath.folder),
                with_slashes(&beneath.path)
            ),
            Target::MountPoint(mount) => format!("the mount {}", self.name_of(&mount.folder)),
            Target::OnMachine(path) => format!("on the machine {}", path.display()),
            Target::Above(folder) => format!("the folder above {}", folder.display()),
            Target::Outside {
                in_folder_above: true,
            } => "outside, in a folder above".to_owned(),
            Target::Outside {
                in_folder_above: false,
            } => "outside".to_owned(),
            #[cfg(windows)]
            Target::Drives => "the drives".to_owned(),
        }
    }

    /// The name of the folder `opened` is.
    fn name_of(&self, opened: &fs::File) -> String {
        let identity = |folder: &fs::File| FileStat::of_file(folder).unwrap().identity;
        let wanted = identity(opened);
        fs::read_dir(self.root.path())
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| identity(&open_folder(&entry.path()).unwrap()) == wanted)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .unwrap_or_else(|| "a folder not made here".to_owned())
    }
}

/// `path` as a program writes it, its names between `/`, which Windows writes `\`.
fn with_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// `path`, an absolute path of this computer, as a program names it from the root, which a
/// resolve takes: `C:\\Users\\note.txt` as `c/Users/note.txt` on Windows.
fn from_root(path: &Path) -> PathBuf {
    let named = machine_paths::in_program(path).unwrap();
    named.strip_prefix("/").unwrap().to_path_buf()
}

fn mount(host: &Path, at: &str) -> Mount {
    Mount {
        host: host.to_owned(),
        at: PathBuf::from(at),
        writable: true,
    }
}

/// Grants whose mounts the test sets, and each pick keeps, each change heard by whoever waits for one.
#[derive(Default)]
struct TestGrants {
    file_system: Mutex<FileSystemGrant>,
    waiting: Mutex<Vec<oneshot::Sender<()>>>,
}

impl TestGrants {
    fn mount(&self, file_system: FileSystemGrant) {
        *self.file_system.lock().unwrap() = file_system;
        for waiting in self.waiting.lock().unwrap().drain(..) {
            let _ = waiting.send(());
        }
    }
}

impl ProgramGrants for TestGrants {
    fn network(&self) -> NetworkGrant {
        NetworkGrant {
            on: false,
            domains: DomainRule::Any,
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
        self.file_system.lock().unwrap().clone()
    }

    fn answer(&self, _question: Question) -> Option<Answer> {
        None
    }

    fn set_answer(&self, _question: Question, _answer: Answer) {}

    fn link_scheme_answer(&self, _scheme: &str) -> Option<Answer> {
        None
    }

    fn set_link_scheme_answer(&self, _scheme: &str, _answer: Answer) {}

    /// The pick joins the mounts, in place of one at its path, as the store keeps it.
    fn keep_picked(&self, pick: Mount) {
        let mut file_system = self.file_system();
        file_system.mounts.retain(|mount| mount.at != pick.at);
        file_system.mounts.push(pick);
        self.mount(file_system);
    }

    fn changed(&self) -> ChangedFuture {
        let (changed, heard) = oneshot::channel();
        self.waiting.lock().unwrap().push(changed);
        Box::pin(async move {
            let _ = heard.await;
        })
    }
}

#[test]
fn a_path_inside_a_mount_is_found_on_the_machine_and_one_leading_out_is_not() {
    let machine = Machine::new(&["docs", "elsewhere"]);
    fs::write(machine.folder("docs").join("note.txt"), "a note").unwrap();
    fs::write(machine.folder("elsewhere").join("secret.txt"), "a secret").unwrap();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(
        machine.folder("elsewhere").join("secret.txt"),
        machine.folder("docs").join("link-out"),
    );
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(
        machine.folder("elsewhere").join("secret.txt"),
        machine.folder("docs").join("link-out"),
    );
    if linked.is_err() {
        // Windows makes symlinks only with Developer Mode on or the privilege to.
        return;
    }
    let table = machine.table(&[("docs", "/mnt/docs")]);
    let note = fs::canonicalize(machine.folder("docs").join("note.txt")).unwrap();
    // The system's own writing of the same file's path: on Windows with the `\\?\` prefix.
    let found = table
        .machine_path(Path::new("/mnt/docs/note.txt"))
        .map(|path| fs::canonicalize(path).unwrap());
    assert_eq!(found.ok(), Some(note));
    assert_eq!(
        table.machine_path(Path::new("/mnt/docs/missing.txt")).ok(),
        None
    );
    assert_eq!(
        table.machine_path(Path::new("/mnt/docs/link-out")).ok(),
        None
    );
    assert_eq!(
        table
            .machine_path(Path::new("/mnt/docs/../docs/note.txt"))
            .ok(),
        None
    );
    assert_eq!(
        table.machine_path(Path::new("/elsewhere/secret.txt")).ok(),
        None
    );
}
