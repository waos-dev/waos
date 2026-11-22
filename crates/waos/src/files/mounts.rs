//! A program's mount table: each folder or file of the machine it sees, opened, at a path inside
//! it, read-only or writable, and each file or folder the user handed it at its own path, what it
//! pasted read-only and what it picked as the dialog asked; where a path of the program leads
//! through them, and where on the machine a path inside a mount is; and the table kept as the
//! program's grants and the files the user handed it hold it now, with its own `/data`, `/cache`
//! and `/tmp` beside them. A pick the grants keep is theirs alone, mounted while they hold it; only
//! one they do not keep is mounted for the run among the files handed. A file picked need not
//! exist: a save's target is mounted before it is written.
//!
//! A path is taken by its names, `.` and `..` included, only while it is above the mounts, where
//! nothing is a symlink, until it is inside the longest mount containing it; the rest is the
//! system's to resolve, beneath that mount's folder, ending as the path ends. So `..` out of a
//! mount is refused there, as `wasmtime-wasi` refuses one out of a preopened folder, and a mount
//! inside another mount is reached through the outer one's path.
//!
//! The invariant: what a table gives is a mount's folder and a path beneath it, a folder above
//! the mounts, or nothing; it never names a folder of the machine outside a mount. A file mounted
//! is reached beneath the folder it is in by its one name, and nothing else of that folder is. A
//! `/` or `/.` after a path's last name, which asks for a folder, stays on the path beneath the
//! mount, which `Path`'s components drop: the calls beneath read a path's end, through the root as
//! from a folder opened beneath the mount.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use tracing::warn;
pub(crate) use waos_capabilities::MountKind;
use waos_capabilities::{ChangedFuture, FileSystemGrant, Mount, ProgramFolders, ProgramGrants};

use super::FilesRefusal;
use super::descriptor::DirectoryEntry;
use super::errors::ErrorCode;
use super::machine_paths;
use super::status::DescriptorType;
use super::target::{Beneath, Target};
use crate::access::ProgramAccess;
use crate::sync::locked;

/// The program's root: the one folder it is given, through which every path of its leads.
pub(crate) const ROOT: &str = "/";

/// Where a program sees its own folders.
const DATA: &str = "/data";
const CACHE: &str = "/cache";
const TMP: &str = "/tmp";

/// What a path beneath a mount keeps of the mount: whether the program may change what is in it,
/// what it is to the program, and where it is on the machine.
#[derive(Clone, Debug)]
pub(crate) struct MountOf {
    pub(crate) writable: bool,
    pub(crate) kind: MountKind,
    pub(crate) place: Arc<Path>,
}

impl MountOf {
    /// A folder granted at `/`, writable as `writable` says, for a test of what is beneath it.
    #[cfg(test)]
    pub(crate) fn granted(writable: bool) -> MountOf {
        MountOf {
            writable,
            kind: MountKind::Granted,
            place: Arc::from(Path::new("/")),
        }
    }

    /// A machine the user gave the program whole, from its root.
    pub(crate) fn machine() -> MountOf {
        MountOf {
            writable: true,
            kind: MountKind::Machine,
            place: Arc::from(Path::new("/")),
        }
    }
}

/// A folder of the machine, or one file of it, opened, at a path inside the program.
#[derive(Clone)]
pub(crate) struct Mounted {
    /// Where the program sees it: an absolute path of names.
    at: PathBuf,
    /// Where it is on the machine.
    host: Arc<Path>,
    /// The folder mounted, or the one the file mounted is in.
    pub(crate) folder: Arc<fs::File>,
    /// For a file mounted, its name in `folder`; none for a folder.
    file: Option<OsString>,
    pub(crate) writable: bool,
    pub(crate) kind: MountKind,
}

impl Mounted {
    /// `host`, a folder or a file, opened at `at` as a mount of `kind`; none, with a warning,
    /// when `at` is no absolute path of names or `host` does not open.
    fn open(host: &Path, at: &Path, writable: bool, kind: MountKind) -> Option<Mounted> {
        let opened = match fs::metadata(host) {
            Ok(metadata) if metadata.is_dir() => open_folder(host).map(|folder| (folder, None)),
            Ok(_) => open_folder_of(host),
            Err(error) => Err(error),
        };
        Mounted::opened(host, at, (writable, kind), opened)
    }

    /// `host`, a folder the user picked, or a file, which may not exist yet, opened at `at` as a
    /// mount of `kind`; none, with a warning, when it does not open.
    fn open_picked(host: &Path, at: &Path, writable: bool, kind: MountKind) -> Option<Mounted> {
        let opened = if host.is_dir() {
            open_folder(host).map(|folder| (folder, None))
        } else {
            open_folder_of(host)
        };
        Mounted::opened(host, at, (writable, kind), opened)
    }

    /// `host`, a folder or a file of the machine, opened read-only at its own path; none, with a
    /// warning, when it does not open.
    fn open_read_only(host: &Path) -> Option<Mounted> {
        let opened = match fs::metadata(host) {
            Ok(metadata) if metadata.is_dir() => open_folder(host).map(|folder| (folder, None)),
            Ok(_) => open_folder_of(host),
            Err(error) => Err(error),
        };
        Mounted::opened(host, &own_path(host)?, (false, MountKind::Pasted), opened)
    }

    /// `opened`, the folder for `host` and the file's name where it is a file, at `at`, writable
    /// and of the kind `access` says.
    fn opened(
        host: &Path,
        at: &Path,
        (writable, kind): (bool, MountKind),
        opened: io::Result<(fs::File, Option<OsString>)>,
    ) -> Option<Mounted> {
        if !is_path_of_names(at) {
            warn!(at = %at.display(), "a mount is not at an absolute path of names; it is left out");
            return None;
        }
        match opened {
            Ok((folder, file)) => Some(Mounted {
                at: at.to_owned(),
                host: Arc::from(host),
                folder: Arc::new(folder),
                file,
                writable,
                kind,
            }),
            Err(error) => {
                warn!(host = %host.display(), %error, "a mount does not open; it is left out");
                None
            }
        }
    }

    /// Where the mount is on the machine.
    pub(crate) fn place(&self) -> &Path {
        &self.host
    }

    /// What a path beneath the mount keeps of it.
    pub(crate) fn of(&self) -> MountOf {
        MountOf {
            writable: self.writable,
            kind: self.kind,
            place: Arc::clone(&self.host),
        }
    }

    /// Whether the program's path `at` is where this mount puts the same path of the machine: a
    /// folder mounted at its own path, `at` beneath it or at it.
    fn reaches(&self, at: &Path) -> bool {
        self.file.is_none()
            && machine_paths::in_program(&self.host).as_deref() == Some(self.at.as_path())
            && at.starts_with(&self.at)
    }

    /// What `within`, a path inside the mount, then `rest` beneath it, names, the path ending in
    /// `ending`.
    fn target(&self, within: &Path, rest: &Path, ending: &OsStr) -> Target {
        // Joining an empty path would add a `/`, which asks for a folder.
        let path = if rest.as_os_str().is_empty() {
            within.to_path_buf()
        } else {
            within.join(rest)
        };
        let names_the_mount = path.components().all(|name| name == Component::CurDir);
        match &self.file {
            Some(name) => Target::Beneath(Beneath {
                folder: Arc::clone(&self.folder),
                path: ending_in(
                    if names_the_mount {
                        PathBuf::from(name)
                    } else {
                        Path::new(name).join(path)
                    },
                    ending,
                ),
                mount: self.of(),
                mounted_file: names_the_mount,
            }),
            None if names_the_mount => Target::MountPoint(self.clone()),
            None => Target::Beneath(Beneath {
                folder: Arc::clone(&self.folder),
                path: ending_in(path, ending),
                mount: self.of(),
                mounted_file: false,
            }),
        }
    }
}

/// Where the program sees `host` at its own path: as it names this computer's path
/// ([`machine_paths`]); none, with a warning, for a path it has no name for.
fn own_path(host: &Path) -> Option<PathBuf> {
    let own = machine_paths::in_program(host);
    if own.is_none() {
        warn!(host = %host.display(), "a file handed has no path a program names; it is left out");
    }
    own
}

/// `path`, ending in `ending` after its last name.
fn ending_in(path: PathBuf, ending: &OsStr) -> PathBuf {
    let mut path = path.into_os_string();
    path.push(ending);
    PathBuf::from(path)
}

/// What `path` ends in after its last name, which its components leave out: a `/` or `/.`, which
/// asks for a folder, or nothing.
fn ending(path: &Path) -> &OsStr {
    let names = path.components().as_path().as_os_str().len();
    let ending = &path.as_os_str().as_encoded_bytes()[names..];
    // A `/` or `/.`, or nothing: ASCII, read as text.
    OsStr::new(std::str::from_utf8(ending).unwrap_or_default())
}

/// The folder the file at `host` is in, opened for paths beneath it, and the file's name.
fn open_folder_of(host: &Path) -> io::Result<(fs::File, Option<OsString>)> {
    let (Some(folder), Some(name)) = (host.parent(), host.file_name()) else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    Ok((open_folder(folder)?, Some(name.to_owned())))
}

/// The folder at `host`, opened for paths beneath it: on Windows as a folder, which it opens only
/// so, and not to be renamed or deleted while it is held; on Linux and Android, where the system
/// refuses WaOS reading it, as Android refuses an app reading `/`, held to pass through alone, so
/// what is beneath it opens as the system allows and listing the folder itself is refused.
pub(crate) fn open_folder(host: &Path) -> io::Result<fs::File> {
    #[cfg(unix)]
    let folder = open_unix_folder(host)?;
    #[cfg(windows)]
    let folder = super::windows::open_folder(host)?;
    if folder.metadata()?.is_dir() {
        Ok(folder)
    } else {
        Err(io::ErrorKind::NotADirectory.into())
    }
}

/// The folder at `host`, opened to read; on Linux and Android, held to pass through alone where the
/// system refuses reading it (`O_PATH`), which the walk beneath it takes as it takes any folder.
#[cfg(unix)]
fn open_unix_folder(host: &Path) -> io::Result<fs::File> {
    let refused = match fs::File::open(host) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => error,
        opened => return opened,
    };
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use rustix::fs::{Mode, OFlags};
        let flags = OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC;
        if let Ok(passed_through) = rustix::fs::open(host, flags, Mode::empty()) {
            return Ok(fs::File::from(passed_through));
        }
    }
    Err(refused)
}

/// Whether `path` is absolute and names only folders, with no `..`; `.` is dropped from a path's
/// components.
fn is_path_of_names(path: &Path) -> bool {
    let mut components = path.components();
    components.next() == Some(Component::RootDir)
        && components.all(|component| matches!(component, Component::Normal(_)))
}

/// A mount as an inspector reads it: where the program sees it, what it is, whether the program
/// may change it, and where it is on the machine.
pub(crate) struct MountView {
    pub(crate) at: PathBuf,
    pub(crate) writable: bool,
    pub(crate) kind: MountKind,
    pub(crate) place: Arc<Path>,
}

/// One entry of a folder as an inspector reads it: its name, whether it is a folder, its size
/// where it is a file, and when it last changed.
pub(crate) struct EntryView {
    pub(crate) name: String,
    pub(crate) is_folder: bool,
    pub(crate) size: Option<u64>,
    pub(crate) modified: Option<(i64, u32)>,
}

/// The mounts a program sees at one time.
pub(crate) struct MountTable {
    /// The deepest first, so the first containing a path is the longest.
    mounts: Vec<Mounted>,
}

impl MountTable {
    /// The table a program sees from its own `folders` and `temporary` folder, its `grants` and
    /// the files handed to it, as its run builds it and an inspector reads it.
    pub(crate) fn of(
        folders: Option<&ProgramFolders>,
        temporary: Option<&Path>,
        grants: &dyn ProgramGrants,
        handed: &HandedFiles,
    ) -> MountTable {
        let own = own_mounts(folders, temporary);
        let granted = grants.file_system();
        MountTable::new(&own, &granted, &handed.now())
    }

    /// Each mount, as an inspector reads it.
    pub(crate) fn mounts(&self) -> Vec<MountView> {
        (self.mounts.iter())
            .map(|mounted| MountView {
                at: mounted.at.clone(),
                writable: mounted.writable,
                kind: mounted.kind,
                place: Arc::clone(&mounted.host),
            })
            .collect()
    }

    /// The entries of `path`, a path of the program with `/` at its root, as the program's own
    /// access resolves it: a folder above the mounts, a mount's own folder, or a folder beneath
    /// one. Never a folder outside a mount.
    pub(crate) fn read_folder(&self, path: &str) -> Result<Vec<EntryView>, ErrorCode> {
        let path = Path::new(path);
        let relative = path.strip_prefix(ROOT).map_err(|_| ErrorCode::Invalid)?;
        // The root itself names the folder the mounts are under.
        let relative = if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        };
        let entries = match self.resolve(Path::new(ROOT), relative)? {
            Target::Above(folder) => self.entries_under(&folder),
            target => target.open_folder()?.read_directory()?,
        };
        let mut entries: Vec<EntryView> = (entries.into_iter())
            .filter_map(|entry| self.entry_view(path, entry))
            .collect();
        entries.sort_by(|left, right| {
            right
                .is_folder
                .cmp(&left.is_folder)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(entries)
    }

    /// `entry`, one of the entries of the program's `folder`, read as an inspector reads it; none
    /// for an entry whose name the program cannot read, which is skipped.
    fn entry_view(
        &self,
        folder: &Path,
        entry: Result<DirectoryEntry, ErrorCode>,
    ) -> Option<EntryView> {
        let entry = entry.ok()?;
        let is_folder = entry.type_ == DescriptorType::Directory;
        let path = folder.join(&entry.name);
        let relative = path.strip_prefix(ROOT).ok()?;
        let stat = self
            .resolve(Path::new(ROOT), relative)
            .and_then(|target| target.status(true))
            .ok();
        let (size, modified) = match stat {
            Some(stat) => (
                (!is_folder).then_some(stat.size),
                stat.data_modification_timestamp
                    .map(|time| (time.seconds, time.nanoseconds)),
            ),
            None => (None, None),
        };
        Some(EntryView {
            name: entry.name,
            is_folder,
            size,
            modified,
        })
    }

    /// `own`, then each of `granted`'s mounts that opens, a pick of the run's kept among them as a
    /// pick opens, then each of the files `handed` for the run alone, but for one at a path a
    /// mount is at already, which is left out, and one handed that the program reaches at its path
    /// already, through a mount of the folder it is in, which keeps that mount's access; then each
    /// folder the program mounted itself, but at a path a mount is at already. The longest mount
    /// containing a path is the one it leads beneath, so the program's own folders stay over a
    /// folder it mounted at `/`.
    fn new(own: &[Mounted], granted: &FileSystemGrant, handed: &Handed) -> MountTable {
        let mut mounts = own.to_vec();
        let opened = (granted.mounts.iter()).filter_map(|mount| handed.open_granted(mount));
        for mount in opened {
            if mounts.iter().any(|mounted| mounted.at == mount.at) {
                warn!(at = %mount.at.display(), "a mount is at a path another is at; it is left out");
                continue;
            }
            mounts.push(mount);
        }
        for mount in handed.for_the_run() {
            let reached = mounts
                .iter()
                .any(|mounted| mounted.at == mount.at || mounted.reaches(&mount.at));
            if !reached {
                mounts.push(mount.clone());
            }
        }
        for (_, mount) in &handed.mounted {
            if !mounts.iter().any(|mounted| mounted.at == mount.at) {
                mounts.push(mount.clone());
            }
        }
        mounts.sort_by_key(|mounted| std::cmp::Reverse(mounted.at.components().count()));
        MountTable { mounts }
    }

    /// Where on the machine `path`, an absolute path of names inside a mount, is, each symlink on
    /// the way followed, as the system's apps name it ([`machine_paths::plain`]); refused by the
    /// mounts for a path outside every mount or where a symlink leads out of the mount, failed
    /// for one that is no path of names or where nothing is.
    pub(crate) fn machine_path(&self, path: &Path) -> Result<PathBuf, FilesRefusal> {
        if !is_path_of_names(path) {
            return Err(FilesRefusal::failed(ErrorCode::Invalid));
        }
        let (mount, within) = self
            .containing(path)
            .ok_or(FilesRefusal::by_the_mounts(ErrorCode::NoEntry))?;
        // Joining an empty path would add a `/`, which asks for a folder.
        let named = if within.as_os_str().is_empty() {
            mount.host.to_path_buf()
        } else {
            mount.host.join(within)
        };
        let nothing_there = |_| FilesRefusal::failed(ErrorCode::NoEntry);
        let machine_path = fs::canonicalize(named).map_err(nothing_there)?;
        let mount_path = fs::canonicalize(&mount.host).map_err(nothing_there)?;
        if machine_path.starts_with(mount_path) {
            Ok(machine_paths::plain(machine_path))
        } else {
            Err(FilesRefusal::by_the_mounts(ErrorCode::NoEntry))
        }
    }

    /// Where `path` leads from `from`, a folder of the program: by its names while above the
    /// mounts, then beneath the longest mount containing it, ending as `path` ends. An empty path
    /// names nothing, and `..` above the root and an absolute path are refused as leaving the
    /// root.
    pub(crate) fn resolve(&self, from: &Path, path: &Path) -> Result<Target, ErrorCode> {
        if path.as_os_str().is_empty() {
            return Err(ErrorCode::NoEntry);
        }
        let mut at = from.to_path_buf();
        let mut names = path.components();
        while !self.leads_beneath(&at) {
            match names.next() {
                None => break,
                Some(Component::CurDir) => {}
                Some(Component::ParentDir) => {
                    if !at.pop() {
                        return Err(ErrorCode::NotPermitted);
                    }
                }
                Some(Component::Normal(name)) => {
                    at.push(name);
                    if self.is_outside(&at) {
                        let in_folder_above = names.as_path().as_os_str().is_empty();
                        return Ok(Target::Outside { in_folder_above });
                    }
                }
                Some(Component::RootDir | Component::Prefix(_)) => {
                    return Err(ErrorCode::NotPermitted);
                }
            }
        }
        Ok(self.target_at(&at, names.as_path(), ending(path)))
    }

    /// What the program's path `at`, then `rest` beneath it, names, the path ending in `ending`.
    fn target_at(&self, at: &Path, rest: &Path, ending: &OsStr) -> Target {
        match self.containing(at) {
            Some((mount, within)) => mount.target(within, rest, ending),
            None if self.is_folder_above(at) => Target::Above(at.to_owned()),
            None => Target::Outside {
                in_folder_above: false,
            },
        }
    }

    /// What a folder above the mounts holds: for the next name of each mount under it, a file
    /// where a file is mounted at that name, and a folder otherwise.
    pub(crate) fn entries_under(&self, folder: &Path) -> Vec<Result<DirectoryEntry, ErrorCode>> {
        self.names_under(folder)
            .into_iter()
            .map(|name| {
                if self.has_file_at(&folder.join(&name)) {
                    DirectoryEntry::file_above(name)
                } else {
                    DirectoryEntry::folder_above(name)
                }
            })
            .collect()
    }

    /// Whether a file is mounted at `at`.
    fn has_file_at(&self, at: &Path) -> bool {
        self.mounts
            .iter()
            .any(|mounted| mounted.at == at && mounted.file.is_some())
    }

    /// The next name of each mount under `folder`, each once, in order.
    fn names_under(&self, folder: &Path) -> Vec<OsString> {
        let mut names: Vec<OsString> = self
            .mounts
            .iter()
            .filter_map(
                |mounted| match mounted.at.strip_prefix(folder).ok()?.components().next() {
                    Some(Component::Normal(name)) => Some(name.to_owned()),
                    _ => None,
                },
            )
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// The longest mount containing `at`, and the rest of `at` within it.
    fn containing<'a>(&self, at: &'a Path) -> Option<(&Mounted, &'a Path)> {
        self.mounts
            .iter()
            .find_map(|mounted| Some((mounted, at.strip_prefix(&mounted.at).ok()?)))
    }

    /// Whether some mount is beneath `at`, short of `at` itself.
    fn has_mount_under(&self, at: &Path) -> bool {
        self.mounts
            .iter()
            .any(|mounted| mounted.at != at && mounted.at.starts_with(at))
    }

    /// Whether `at` is a folder above the mounts: the root, or a folder a mount is under.
    fn is_folder_above(&self, at: &Path) -> bool {
        at == Path::new(ROOT) || self.has_mount_under(at)
    }

    /// Whether what follows `at` is the system's to resolve: `at` is inside a mount, and no
    /// longer mount is under it.
    fn leads_beneath(&self, at: &Path) -> bool {
        self.containing(at).is_some() && !self.has_mount_under(at)
    }

    /// Whether `at` is outside every mount and above none.
    fn is_outside(&self, at: &Path) -> bool {
        self.containing(at).is_none() && !self.is_folder_above(at)
    }
}

/// A program's mount table as its grants and the files the user handed it hold it now: rebuilt
/// when the grants change and mount other folders, or the user hands it a file, so a mount added
/// or removed applies at the next call.
pub(crate) struct LiveMounts {
    /// Where the grants are read, which the table is compiled from.
    access: ProgramAccess,
    /// The program's own folders, opened once for its run.
    own: Vec<Mounted>,
    handed: HandedFiles,
    /// What the table was built from: the grants, and how many handings made the files handed.
    granted: FileSystemGrant,
    handings: u64,
    table: MountTable,
    /// The grants' next change.
    changed: ChangedFuture,
}

impl LiveMounts {
    /// The mounts of a program with its own `folders` and its `temporary` folder, where it has
    /// them, the grants `access` reads, and the files the user hands it, `handed`.
    pub(crate) fn new(
        folders: Option<&ProgramFolders>,
        temporary: Option<&Path>,
        access: ProgramAccess,
        handed: HandedFiles,
    ) -> LiveMounts {
        let own = own_mounts(folders, temporary);
        let changed = access.grants().changed();
        let granted = access.grants().file_system();
        let handed_now = handed.now();
        let table = MountTable::new(&own, &granted, &handed_now);
        LiveMounts {
            access,
            own,
            handed,
            granted,
            handings: handed_now.handings,
            table,
            changed,
        }
    }

    /// The table as the grants and the files handed hold it now. The next change of the grants
    /// is asked for before they are read, so a change between the two is heard at the next call;
    /// and they are read again at a handing, which a pick they keep comes with.
    pub(crate) fn now(&mut self) -> &MountTable {
        let grants_changed = (&mut self.changed).now_or_never().is_some();
        if grants_changed {
            self.changed = self.access.grants().changed();
        }
        let handed_changed = self.handed.handings() != self.handings;
        if grants_changed || handed_changed {
            let granted = self.access.grants().file_system();
            if handed_changed || granted != self.granted {
                let handed = self.handed.now();
                self.table = MountTable::new(&self.own, &granted, &handed);
                self.granted = granted;
                self.handings = handed.handings;
            }
        }
        &self.table
    }
}

/// The files and folders the user handed a program for its run: what it pasted, read-only at its
/// own path, and what it picked, which the grants keep where they can. They stay for the program's
/// run. Every clone is the same.
#[derive(Clone, Default)]
pub(crate) struct HandedFiles(Arc<Mutex<Handed>>);

/// The files handed, and how many handings made them, which a table built from them compares.
#[derive(Clone, Default)]
struct Handed {
    pasted: Vec<Mounted>,
    picked: Vec<Picked>,
    /// The folders the program mounted itself, by the number each was mounted as.
    mounted: Vec<(u64, Mounted)>,
    next_mount: u64,
    handings: u64,
}

/// A file or folder the user picked during the program's run.
#[derive(Clone)]
enum Picked {
    /// Kept among the grants, which mount it while they hold it: where it is on the machine, a
    /// file there perhaps not made yet.
    Kept(PathBuf),
    /// Mounted for the run alone, where the grants keep nothing.
    ForTheRun(Mounted),
}

impl Picked {
    fn host(&self) -> &Path {
        match self {
            Picked::Kept(host) => host,
            Picked::ForTheRun(mounted) => &mounted.host,
        }
    }
}

impl HandedFiles {
    /// Mounts `host`, a file or a folder of the machine the user pasted, read-only at its own
    /// path; nothing for one mounted already, or that does not open.
    pub(crate) fn hand(&self, host: &Path) {
        let mut handed = locked(&self.0);
        if handed.pasted.iter().any(|mounted| *mounted.host == *host) {
            return;
        }
        handed.pasted.extend(Mounted::open_read_only(host));
        handed.handings += 1;
    }

    /// Takes `pick`, a file or a folder the user picked, at its own path, in place of an earlier
    /// pick of it: one the grants keep, `kept`, for them to mount; any other mounted for the run,
    /// a file that need not exist yet. Nothing for one that does not open.
    pub(crate) fn hand_pick(&self, pick: &Mount, kept: bool) {
        let mut handed = locked(&self.0);
        handed.picked.retain(|picked| picked.host() != pick.host);
        let picked = if kept {
            Some(Picked::Kept(pick.host.clone()))
        } else {
            Mounted::open_picked(&pick.host, &pick.at, pick.writable, MountKind::Picked)
                .map(Picked::ForTheRun)
        };
        handed.picked.extend(picked);
        handed.handings += 1;
    }

    /// Mounts `host`, a folder the program holds, at `at` in its own files for its run, writable as
    /// `writable` says and of the `kind` its folder is, and answers the number to unmount it by:
    /// `invalid` for no absolute path of names, `not-permitted` inside the program's own folders,
    /// which stay its own, `exist` where it mounted a folder already, and `no-entry` for one that
    /// does not open (`wasdk:platform/mounts`).
    pub(crate) fn mount(
        &self,
        host: &Path,
        at: &Path,
        writable: bool,
        kind: MountKind,
    ) -> Result<u64, ErrorCode> {
        if !is_path_of_names(at) {
            return Err(ErrorCode::Invalid);
        }
        if [DATA, CACHE, TMP].iter().any(|own| at.starts_with(own)) {
            return Err(ErrorCode::NotPermitted);
        }
        let mut handed = locked(&self.0);
        if handed.mounted.iter().any(|(_, mounted)| *mounted.at == *at) {
            return Err(ErrorCode::Exist);
        }
        let mounted = Mounted::open(host, at, writable, kind).ok_or(ErrorCode::NoEntry)?;
        let number = handed.next_mount;
        handed.next_mount += 1;
        handed.mounted.push((number, mounted));
        handed.handings += 1;
        Ok(number)
    }

    /// Takes away the folder the program mounted as `number`.
    pub(crate) fn unmount(&self, number: u64) {
        let mut handed = locked(&self.0);
        handed.mounted.retain(|(mounted, _)| *mounted != number);
        handed.handings += 1;
    }

    /// The files handed now.
    fn now(&self) -> Handed {
        locked(&self.0).clone()
    }

    /// How many handings made the files handed now.
    fn handings(&self) -> u64 {
        locked(&self.0).handings
    }
}

impl Handed {
    /// `mount`, one of the grants', opened: as a pick of the run is, a file that need not exist
    /// yet, where it is one the grants keep; none, with a warning, where it does not open.
    fn open_granted(&self, mount: &Mount) -> Option<Mounted> {
        let picked = self
            .picked
            .iter()
            .any(|picked| matches!(picked, Picked::Kept(host) if *host == mount.host));
        let open = if picked {
            Mounted::open_picked
        } else {
            Mounted::open
        };
        open(&mount.host, &mount.at, mount.writable, MountKind::Granted)
    }

    /// The files mounted for the run alone: those pasted, and the picks the grants do not keep.
    fn for_the_run(&self) -> impl Iterator<Item = &Mounted> {
        let picked = self.picked.iter().filter_map(|picked| match picked {
            Picked::ForTheRun(mounted) => Some(mounted),
            Picked::Kept(_) => None,
        });
        self.pasted.iter().chain(picked)
    }
}

/// The program's own folders, writable, at `/data` and `/cache`, and its temporary folder at
/// `/tmp`, each where it has it.
fn own_mounts(folders: Option<&ProgramFolders>, temporary: Option<&Path>) -> Vec<Mounted> {
    let own_folders = folders.into_iter().flat_map(|folders| {
        [
            (folders.data.as_path(), DATA, MountKind::OwnData),
            (folders.cache.as_path(), CACHE, MountKind::OwnCache),
        ]
    });
    own_folders
        .chain(temporary.map(|temporary| (temporary, TMP, MountKind::OwnTemporary)))
        .filter_map(|(host, at, kind)| Mounted::open(host, Path::new(at), true, kind))
        .collect()
}

#[cfg(test)]
mod tests;
