//! A program's mount table: each folder or file of the machine it sees, opened, at a path inside
//! it, read-only or writable, and each file or folder the user handed it at its own path, what it
//! pasted read-only and what it picked as the dialog asked; where a path of the program leads
//! through them, and where on the machine a path inside a mount is; and the table kept as the
//! program's grants and the files the user handed it hold it now, with its own `/data` and
//! `/cache` beside them. A file mounted need not exist: a save's target is mounted before it is
//! written.
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
use waos_capabilities::{ChangedFuture, FileSystemGrant, ProgramFolders, ProgramGrants};

use super::descriptor::DirectoryEntry;
use super::errors::ErrorCode;
use super::machine_paths;
use super::target::{Beneath, Target};
use crate::sync::locked;

/// The program's root: the one folder it is given, through which every path of its leads.
pub(crate) const ROOT: &str = "/";

/// Where a program sees its own folders.
const DATA: &str = "/data";
const CACHE: &str = "/cache";

/// A folder of the machine, or one file of it, opened, at a path inside the program.
#[derive(Clone)]
pub(crate) struct Mounted {
    /// Where the program sees it: an absolute path of names.
    at: PathBuf,
    /// Where it is on the machine.
    host: PathBuf,
    /// The folder mounted, or the one the file mounted is in.
    pub(crate) folder: Arc<fs::File>,
    /// For a file mounted, its name in `folder`; none for a folder.
    file: Option<OsString>,
    pub(crate) writable: bool,
}

impl Mounted {
    /// `host`, a folder or a file, opened at `at`; none, with a warning, when `at` is no absolute
    /// path of names or `host` does not open.
    fn open(host: &Path, at: &Path, writable: bool) -> Option<Mounted> {
        let opened = match fs::metadata(host) {
            Ok(metadata) if metadata.is_dir() => open_folder(host).map(|folder| (folder, None)),
            Ok(_) => open_folder_of(host),
            Err(error) => Err(error),
        };
        Mounted::opened(host, at, writable, opened)
    }

    /// `host`, a folder the user picked, or a file, which may not exist yet, opened at its own
    /// path; none, with a warning, when it does not open.
    fn open_picked(host: &Path, writable: bool) -> Option<Mounted> {
        let opened = if host.is_dir() {
            open_folder(host).map(|folder| (folder, None))
        } else {
            open_folder_of(host)
        };
        Mounted::opened(host, &own_path(host)?, writable, opened)
    }

    /// `host`, a folder or a file of the machine, opened read-only at its own path; none, with a
    /// warning, when it does not open.
    fn open_read_only(host: &Path) -> Option<Mounted> {
        let opened = match fs::metadata(host) {
            Ok(metadata) if metadata.is_dir() => open_folder(host).map(|folder| (folder, None)),
            Ok(_) => open_folder_of(host),
            Err(error) => Err(error),
        };
        Mounted::opened(host, &own_path(host)?, false, opened)
    }

    /// `opened`, the folder for `host` and the file's name where it is a file, at `at`.
    fn opened(
        host: &Path,
        at: &Path,
        writable: bool,
        opened: io::Result<(fs::File, Option<OsString>)>,
    ) -> Option<Mounted> {
        if !is_path_of_names(at) {
            warn!(at = %at.display(), "a mount is not at an absolute path of names; it is left out");
            return None;
        }
        match opened {
            Ok((folder, file)) => Some(Mounted {
                at: at.to_owned(),
                host: host.to_owned(),
                folder: Arc::new(folder),
                file,
                writable,
            }),
            Err(error) => {
                warn!(host = %host.display(), %error, "a mount does not open; it is left out");
                None
            }
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
                writable: self.writable,
                mounted_file: names_the_mount,
            }),
            None if names_the_mount => Target::MountPoint(self.clone()),
            None => Target::Beneath(Beneath {
                folder: Arc::clone(&self.folder),
                path: ending_in(path, ending),
                writable: self.writable,
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
/// so, and not to be renamed or deleted while it is held.
pub(crate) fn open_folder(host: &Path) -> io::Result<fs::File> {
    #[cfg(unix)]
    let folder = fs::File::open(host)?;
    #[cfg(windows)]
    let folder = super::windows::open_folder(host)?;
    if folder.metadata()?.is_dir() {
        Ok(folder)
    } else {
        Err(io::ErrorKind::NotADirectory.into())
    }
}

/// Whether `path` is absolute and names only folders, with no `..`; `.` is dropped from a path's
/// components.
fn is_path_of_names(path: &Path) -> bool {
    let mut components = path.components();
    components.next() == Some(Component::RootDir)
        && components.all(|component| matches!(component, Component::Normal(_)))
}

/// The mounts a program sees at one time.
pub(crate) struct MountTable {
    /// The deepest first, so the first containing a path is the longest.
    mounts: Vec<Mounted>,
}

impl MountTable {
    /// `own`, then each of `granted`'s mounts that opens, then each of `handed`, but for one at a
    /// path a mount is at already, which is left out, and one handed that the program reaches at
    /// its path already, through a mount of the folder it is in, which keeps that mount's access:
    /// a pick kept among the grants is the grants' from then on.
    fn new(own: &[Mounted], granted: &FileSystemGrant, handed: &[Mounted]) -> MountTable {
        let mut mounts = own.to_vec();
        let opened = granted
            .mounts
            .iter()
            .filter_map(|mount| Mounted::open(&mount.host, &mount.at, mount.writable));
        for mount in opened {
            if mounts.iter().any(|mounted| mounted.at == mount.at) {
                warn!(at = %mount.at.display(), "a mount is at a path another is at; it is left out");
                continue;
            }
            mounts.push(mount);
        }
        for mount in handed {
            let reached = mounts
                .iter()
                .any(|mounted| mounted.at == mount.at || mounted.reaches(&mount.at));
            if !reached {
                mounts.push(mount.clone());
            }
        }
        mounts.sort_by_key(|mounted| std::cmp::Reverse(mounted.at.components().count()));
        MountTable { mounts }
    }

    /// Where on the machine `path`, an absolute path of names inside a mount, is, each symlink on
    /// the way followed, as the system's apps name it ([`machine_paths::plain`]); none for a path
    /// outside every mount, where nothing is, or where a symlink leads out of the mount.
    pub(crate) fn machine_path(&self, path: &Path) -> Option<PathBuf> {
        if !is_path_of_names(path) {
            return None;
        }
        let (mount, within) = self.containing(path)?;
        // Joining an empty path would add a `/`, which asks for a folder.
        let named = if within.as_os_str().is_empty() {
            mount.host.clone()
        } else {
            mount.host.join(within)
        };
        let machine_path = fs::canonicalize(named).ok()?;
        let mount_path = fs::canonicalize(&mount.host).ok()?;
        machine_path
            .starts_with(mount_path)
            .then(|| machine_paths::plain(machine_path))
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
    grants: Arc<dyn ProgramGrants>,
    /// The program's own folders, opened once for its run.
    own: Vec<Mounted>,
    handed: HandedFiles,
    /// What the table was built from: the grants, and the files handed by then.
    granted: FileSystemGrant,
    handings: u64,
    table: MountTable,
    /// The grants' next change.
    changed: ChangedFuture,
}

impl LiveMounts {
    /// The mounts of a program with its own `folders`, where it has them, `grants`, and the
    /// files the user hands it, `handed`.
    pub(crate) fn new(
        folders: Option<&ProgramFolders>,
        grants: Arc<dyn ProgramGrants>,
        handed: HandedFiles,
    ) -> LiveMounts {
        let own = folders.map(own_mounts).unwrap_or_default();
        let changed = grants.changed();
        let granted = grants.file_system();
        let (handed_now, handings) = handed.mounts();
        let table = MountTable::new(&own, &granted, &handed_now);
        LiveMounts {
            grants,
            own,
            handed,
            granted,
            handings,
            table,
            changed,
        }
    }

    /// The table as the grants and the files handed hold it now. The next change of the grants
    /// is asked for before they are read, so a change between the two is heard at the next call.
    pub(crate) fn now(&mut self) -> &MountTable {
        let mut granted = None;
        if (&mut self.changed).now_or_never().is_some() {
            self.changed = self.grants.changed();
            granted = Some(self.grants.file_system()).filter(|now| *now != self.granted);
        }
        let (handed, handings) = self.handed.mounts();
        if granted.is_some() || handings != self.handings {
            if let Some(granted) = granted {
                self.granted = granted;
            }
            self.table = MountTable::new(&self.own, &self.granted, &handed);
            self.handings = handings;
        }
        &self.table
    }
}

/// The files and folders the user handed a program, each mounted at its own path: what the user
/// pasted, read-only, and what it picked in a dialog, writable where the dialog asked. They stay
/// for the program's run. Every clone is the same.
#[derive(Clone, Default)]
pub(crate) struct HandedFiles(Arc<Mutex<Handed>>);

/// The files handed, and how many handings made them, which a table built from them compares.
#[derive(Default)]
struct Handed {
    mounts: Vec<Mounted>,
    handings: u64,
}

impl HandedFiles {
    /// Mounts `host`, a file or a folder of the machine the user pasted, read-only at its own
    /// path; nothing for one mounted already, or that does not open.
    pub(crate) fn hand(&self, host: &Path) {
        let mut handed = locked(&self.0);
        if handed.mounts.iter().any(|mounted| mounted.host == host) {
            return;
        }
        handed.mounts.extend(Mounted::open_read_only(host));
        handed.handings += 1;
    }

    /// Mounts `host`, a file or a folder the user picked, at its own path, writable as `writable`
    /// says, in place of an earlier handing of it; a file need not exist yet. Nothing for one that
    /// does not open.
    pub(crate) fn hand_pick(&self, host: &Path, writable: bool) {
        let mut handed = locked(&self.0);
        handed.mounts.retain(|mounted| mounted.host != host);
        handed.mounts.extend(Mounted::open_picked(host, writable));
        handed.handings += 1;
    }

    /// The files handed now, and how many handings made them.
    fn mounts(&self) -> (Vec<Mounted>, u64) {
        let handed = locked(&self.0);
        (handed.mounts.clone(), handed.handings)
    }
}

/// The program's own folders, writable, at `/data` and `/cache`.
fn own_mounts(folders: &ProgramFolders) -> Vec<Mounted> {
    [(&folders.data, DATA), (&folders.cache, CACHE)]
        .into_iter()
        .filter_map(|(host, at)| Mounted::open(host, Path::new(at), true))
        .collect()
}

#[cfg(test)]
mod tests;
