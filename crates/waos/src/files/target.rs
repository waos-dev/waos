//! Where a path given with a descriptor leads, and the calls that name one: an open, a status, a
//! symlink read, and each change of the folders, refused where the mount or the table does not
//! allow it.
//!
//! What a change is refused with, by where it is made and what it does:
//!
//! | where | create | replace | remove | modify |
//! | --- | --- | --- | --- | --- |
//! | beneath a read-only mount | `read-only` | `read-only` | `read-only` | `read-only` |
//! | a mount's own folder | `exist` | `busy` | `busy` | as its mount allows |
//! | a folder above the mounts | `exist` | `busy` | `busy` | `read-only` |
//! | a name a folder above lacks | `read-only` | `read-only` | `no-entry` | `no-entry` |
//! | anywhere else outside the mounts | `no-entry` | `no-entry` | `no-entry` | `no-entry` |
//!
//! A path from a machine's root ([`Target::OnMachine`]) is the machine's, as any program on it
//! reaches it (the processes proposal's decision 8): it is opened and its status read through the
//! system's ordinary calls, which follow every symlink and `..` as they lead, and a change is made
//! in the folder that holds what it names, opened as the system resolves it.
//!
//! The table, and what an open is refused with, are callable apart from making the change or the
//! open ([`Target::refusal_for`], [`Target::open_refusal`]), so the program's access decides a use
//! by them before it is made (`files`).
//!
//! The invariant: from a mount, the machine is changed only beneath a writable mount's folder; a
//! folder above a mount and a mount's own folder are the table's, which only the user changes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

#[cfg(unix)]
use rustix::fs::{Mode, OFlags};

use super::beneath::{self, FollowSymlinks, OpenOptions};
use super::descriptor::{Descriptor, OpenMode, Opened};
use super::errors::ErrorCode;
use super::mounts::{MountKind, Mounted, open_folder};
use super::status::{DescriptorStat, FileStat, MetadataHashValue};
use crate::access::{Resolution, Resolved};

/// Where a path given with a descriptor leads.
pub(crate) enum Target {
    /// A path beneath a folder of the machine: a mount's, or one opened beneath a mount.
    Beneath(Beneath),
    /// A path of a machine the user gave the program whole, from its root, as the machine
    /// resolves it.
    OnMachine(PathBuf),
    /// A mount's own folder, named from above it.
    MountPoint(Mounted),
    /// A folder above the mounts, by its path in the program.
    Above(PathBuf),
    /// Outside every mount and above none: `in_folder_above` when the path's last name is all a
    /// folder above the mounts lacks.
    Outside { in_folder_above: bool },
    /// On Windows, this computer's root as a machine, the folder of its drives.
    #[cfg(windows)]
    Drives,
}

/// A path beneath a folder of the machine, and whether its mount lets the program change it.
pub(crate) struct Beneath {
    pub(crate) folder: Arc<fs::File>,
    pub(crate) path: PathBuf,
    pub(crate) writable: bool,
    /// The kind of the mount the folder is in.
    pub(crate) kind: MountKind,
    /// Whether the path names a file mounted itself, which a rename may replace from another
    /// volume.
    pub(crate) mounted_file: bool,
}

/// What an open asks for: WASI's path flags, open flags and descriptor flags, as one.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OpenRequest {
    /// Whether a symlink the path ends in is followed.
    pub(crate) follow: bool,
    pub(crate) create: bool,
    /// With `create`, whether the file must be new.
    pub(crate) exclusive: bool,
    pub(crate) truncate: bool,
    /// Whether what is opened must be a folder.
    pub(crate) directory: bool,
    pub(crate) read: bool,
    pub(crate) write: bool,
    /// Whether any of WASI's synchronized I/O is asked for, which no open serves.
    pub(crate) synchronized: bool,
}

/// What a call does to the path it names, by which a refusal is chosen.
#[derive(Clone, Copy)]
pub(super) enum Change {
    /// Makes something new at the path.
    Create,
    /// Puts something at the path, in place of what may be there.
    Replace,
    /// Takes away, or moves away, what is at the path.
    Remove,
    /// Changes what is at the path, in place.
    Modify,
}

impl Target {
    /// Where `path`, given with a machine's root on Windows, leads: the drives' folder itself,
    /// `/`, where the path names no drive; a drive's path by its letter, `c/Users`, which the
    /// machine resolves from there, `..` and symlinks as they lead; and nothing outside the
    /// drives. A `..` above the drives stays at them, as one above `/` does.
    #[cfg(windows)]
    pub(crate) fn in_drives(path: &str) -> Target {
        let mut names = path
            .split('/')
            .skip_while(|name| matches!(*name, "" | "." | ".."));
        let Some(drive) = names.next() else {
            return Target::Drives;
        };
        let rest: Vec<&str> = names.collect();
        let program = format!("/{drive}/{}", rest.join("/"));
        match super::machine_paths::on_this_computer(Path::new(&program)) {
            Some(machine) => Target::OnMachine(machine),
            None => Target::Outside {
                in_folder_above: rest.is_empty(),
            },
        }
    }

    /// Opens what the path names, as `request` asks.
    pub(crate) fn open(&self, request: &OpenRequest) -> Result<Descriptor, ErrorCode> {
        request.check()?;
        match self {
            Target::Beneath(beneath) => open_beneath(
                &beneath.folder,
                &beneath.path,
                (beneath.writable, beneath.kind),
                request,
            ),
            Target::MountPoint(mount) => open_beneath(
                &mount.folder,
                Path::new("."),
                (mount.writable, mount.kind),
                request,
            ),
            Target::OnMachine(path) => open_on_machine(path, request),
            Target::Above(path) => {
                request.check_folder_above()?;
                Ok(Descriptor::Above(path.clone()))
            }
            #[cfg(windows)]
            Target::Drives => {
                request.check_folder_above()?;
                Ok(Descriptor::Drives)
            }
            Target::Outside { in_folder_above } if request.create && *in_folder_above => {
                Err(ErrorCode::ReadOnly)
            }
            Target::Outside { .. } => Err(ErrorCode::NoEntry),
        }
    }

    /// What an open of the path, as `request` asks, is refused with by the mounts, made or not:
    /// outside every mount, as [`open`](Self::open) answers it; one that would change what a
    /// read-only mount holds; and, of a folder above the mounts, one that would make it new or
    /// change it. None where the mounts let it be made.
    pub(crate) fn open_refusal(&self, request: &OpenRequest) -> Option<ErrorCode> {
        match self {
            Target::Beneath(Beneath { writable, .. })
            | Target::MountPoint(Mounted { writable, .. })
                if !writable && request.changes_the_file() =>
            {
                Some(ErrorCode::ReadOnly)
            }
            Target::Above(_) => request.check_folder_above().err(),
            #[cfg(windows)]
            Target::Drives => request.check_folder_above().err(),
            Target::Outside { in_folder_above } if request.create && *in_folder_above => {
                Some(ErrorCode::ReadOnly)
            }
            Target::Outside { .. } => Some(ErrorCode::NoEntry),
            Target::Beneath(_) | Target::MountPoint(_) | Target::OnMachine(_) => None,
        }
    }

    /// What `change` at the path is refused with by the module's table, made or not; none where
    /// the table lets it be made.
    pub(super) fn refusal_for(&self, change: Change) -> Option<ErrorCode> {
        match (self, change) {
            (Target::Beneath(beneath), _) if beneath.writable => None,
            (Target::MountPoint(mount), Change::Modify) if mount.writable => None,
            (Target::OnMachine(_), _) => None,
            (Target::Beneath(_), _)
            | (Target::MountPoint(_) | Target::Above(_), Change::Modify)
            | (
                Target::Outside {
                    in_folder_above: true,
                },
                Change::Create | Change::Replace,
            ) => Some(ErrorCode::ReadOnly),
            (Target::MountPoint(_) | Target::Above(_), Change::Create) => Some(ErrorCode::Exist),
            (Target::MountPoint(_) | Target::Above(_), Change::Replace | Change::Remove) => {
                Some(ErrorCode::Busy)
            }
            // Nothing changes the drives, as nothing changes a folder above the mounts.
            #[cfg(windows)]
            (Target::Drives, Change::Modify) => Some(ErrorCode::ReadOnly),
            #[cfg(windows)]
            (Target::Drives, Change::Create) => Some(ErrorCode::Exist),
            #[cfg(windows)]
            (Target::Drives, Change::Replace | Change::Remove) => Some(ErrorCode::Busy),
            (Target::Outside { .. }, _) => Some(ErrorCode::NoEntry),
        }
    }

    /// Opens the folder the path names, for reading what it holds.
    pub(crate) fn open_folder(&self) -> Result<Opened, ErrorCode> {
        match self.open(&OpenRequest::folder())? {
            Descriptor::Dir(folder) => Ok(folder),
            Descriptor::File(_) | Descriptor::Above(_) => Err(ErrorCode::NotDirectory),
            #[cfg(windows)]
            Descriptor::Drives => Err(ErrorCode::NotDirectory),
        }
    }

    /// The status of what the path names, or of the symlink it ends in without `follow`.
    pub(crate) fn status(&self, follow: bool) -> Result<DescriptorStat, ErrorCode> {
        match self {
            Target::Above(_) => Ok(DescriptorStat::folder_above()),
            #[cfg(windows)]
            Target::Drives => Ok(DescriptorStat::folder_above()),
            _ => Ok(self.file_status(follow)?.status),
        }
    }

    /// The hash of what the path names: of its identity, or of its path above the mounts.
    pub(crate) fn metadata_hash(&self, follow: bool) -> Result<MetadataHashValue, ErrorCode> {
        match self {
            Target::Above(path) => Ok(MetadataHashValue::of_folder_above(path)),
            #[cfg(windows)]
            Target::Drives => Ok(MetadataHashValue::of_folder_above(Path::new(
                super::machine_paths::ROOT,
            ))),
            _ => Ok(MetadataHashValue::of_file(
                self.file_status(follow)?.identity,
            )),
        }
    }

    /// What the symlink at the path holds.
    pub(crate) fn read_link(&self) -> Result<String, ErrorCode> {
        match self {
            Target::Beneath(beneath) => {
                let contents = beneath::read_link(&beneath.folder, &beneath.path)?;
                contents
                    .into_os_string()
                    .into_string()
                    .map_err(|_| ErrorCode::IllegalByteSequence)
            }
            Target::OnMachine(path) => as_the_program_names(&fs::read_link(path)?)
                .into_os_string()
                .into_string()
                .map_err(|_| ErrorCode::IllegalByteSequence),
            Target::MountPoint(_) | Target::Above(_) => Err(ErrorCode::Invalid),
            #[cfg(windows)]
            Target::Drives => Err(ErrorCode::Invalid),
            Target::Outside { .. } => Err(ErrorCode::NoEntry),
        }
    }

    pub(crate) fn create_directory(&self) -> Result<(), ErrorCode> {
        let (folder, path) = self.place_for(Change::Create)?;
        Ok(beneath::create_dir(&folder, &path)?)
    }

    pub(crate) fn remove_directory(&self) -> Result<(), ErrorCode> {
        let (folder, path) = self.place_for(Change::Remove)?;
        Ok(beneath::remove_dir(&folder, &path)?)
    }

    /// Removes the file or the symlink at the path: on Windows a folder's symlink as a folder, as
    /// Windows removes it only so.
    pub(crate) fn unlink_file(&self) -> Result<(), ErrorCode> {
        let (folder, path) = self.place_for(Change::Remove)?;
        #[cfg(unix)]
        return Ok(beneath::remove_file(&folder, &path)?);
        #[cfg(windows)]
        return Ok(super::windows::remove_file_or_symlink(&folder, &path)?);
    }

    /// Makes a symlink at the path, holding `contents`: on a machine, whatever it holds, a path from
    /// the machine's root among them, which [`beneath`] refuses beneath a mount.
    /// On Windows a symlink is a folder's or a file's, by what `contents` names when it is made.
    pub(crate) fn symlink(&self, contents: &str) -> Result<(), ErrorCode> {
        if let Target::OnMachine(path) = self {
            return Ok(symlink_on_machine(contents, path)?);
        }
        let (folder, path) = self.place_for(Change::Create)?;
        #[cfg(unix)]
        return Ok(beneath::symlink(Path::new(contents), &folder, &path)?);
        #[cfg(windows)]
        return Ok(super::windows::symlink(
            Path::new(contents),
            &folder,
            &path,
        )?);
    }

    /// Sets the times of what the path names, or of the symlink it ends in without `follow`;
    /// a time of none is left as it is.
    pub(crate) fn set_times(
        &self,
        follow: bool,
        accessed: Option<SystemTime>,
        modified: Option<SystemTime>,
    ) -> Result<(), ErrorCode> {
        if let Target::MountPoint(mount) = self {
            return Opened::of_mount(mount).set_times(accessed, modified);
        }
        let (folder, path) = match self {
            // What the path leads to, wherever that is on the machine.
            Target::OnMachine(path) if follow => pinned(&fs::canonicalize(path)?)?,
            _ => self.place_for(Change::Modify)?,
        };
        if follow {
            beneath::set_times(&folder, &path, accessed, modified)?;
        } else {
            beneath::set_times_nofollow(&folder, &path, accessed, modified)?;
        }
        Ok(())
    }

    /// Moves what the path names to `to`, in place of what may be there. Onto a file mounted, a
    /// rename from another volume, which the system refuses, is one replace all the same: the file
    /// is copied beside the target first (the file dialogs proposal's decision 3).
    pub(crate) fn rename_to(&self, to: &Target) -> Result<(), ErrorCode> {
        let (from_folder, from_path) = self.place_for(Change::Remove)?;
        let (to_folder, to_path) = to.place_for(Change::Replace)?;
        match beneath::rename(&from_folder, &from_path, &to_folder, &to_path) {
            Err(error) if error.kind() == io::ErrorKind::CrossesDevices && to.is_mounted_file() => {
                replace_across_volumes(&from_folder, &from_path, &to_folder, &to_path)
            }
            renamed => Ok(renamed?),
        }
    }

    /// Whether the path names a file mounted itself.
    fn is_mounted_file(&self) -> bool {
        matches!(self, Target::Beneath(beneath) if beneath.mounted_file)
    }

    /// Makes `to` a hard link to what the path names.
    pub(crate) fn link_to(&self, to: &Target) -> Result<(), ErrorCode> {
        let (from_folder, from_path) = self.place_for(Change::Modify)?;
        let (to_folder, to_path) = to.place_for(Change::Create)?;
        Ok(beneath::hard_link(
            &from_folder,
            &from_path,
            &to_folder,
            &to_path,
        )?)
    }

    /// The status the system gives of what the path names.
    fn file_status(&self, follow: bool) -> Result<FileStat, ErrorCode> {
        match self {
            Target::Beneath(beneath) => Ok(FileStat::beneath(
                &beneath.folder,
                &beneath.path,
                following(follow),
            )?),
            Target::MountPoint(mount) => Opened::of_mount(mount).file_status(),
            Target::OnMachine(path) => Ok(FileStat::on_machine(path, follow)?),
            Target::Above(_) | Target::Outside { .. } => Err(ErrorCode::NoEntry),
            #[cfg(windows)]
            Target::Drives => Err(ErrorCode::NoEntry),
        }
    }

    /// The folder and the path beneath it where `change` is made, or why it is refused (the
    /// module's table): on a machine, the folder holding what the path names, and its name.
    fn place_for(&self, change: Change) -> Result<(Arc<fs::File>, PathBuf), ErrorCode> {
        if let Some(refused) = self.refusal_for(change) {
            return Err(refused);
        }
        match self {
            Target::Beneath(beneath) => Ok((Arc::clone(&beneath.folder), beneath.path.clone())),
            Target::MountPoint(mount) => Ok((Arc::clone(&mount.folder), PathBuf::from("."))),
            Target::OnMachine(path) => pinned(path),
            // The table refuses every change of these.
            Target::Above(_) | Target::Outside { .. } => Err(ErrorCode::NoEntry),
            #[cfg(windows)]
            Target::Drives => Err(ErrorCode::NoEntry),
        }
    }
}

impl Resolved for Target {
    /// Beneath a mount's folder, or a mount's own folder, the place in it; a machine's path; and
    /// nothing for a folder above the mounts and a path outside them.
    fn resolution(&self) -> Resolution<'_> {
        match self {
            Target::Beneath(beneath) => Resolution::File {
                kind: beneath.kind,
                folder: &beneath.folder,
                beneath: &beneath.path,
            },
            Target::MountPoint(mount) => Resolution::File {
                kind: mount.kind,
                folder: &mount.folder,
                beneath: Path::new("."),
            },
            Target::OnMachine(path) => Resolution::OnMachine(path),
            Target::Above(_) | Target::Outside { .. } => Resolution::Nothing,
            #[cfg(windows)]
            Target::Drives => Resolution::Nothing,
        }
    }
}

/// Moves the file at `from` beneath `from_folder` onto `to` beneath `to_folder`, on another
/// volume, as one replace: copied beside the target under a name of its own and made durable,
/// renamed onto the target, then removed where it was. A copy that fails leaves the target as it
/// was.
fn replace_across_volumes(
    from_folder: &fs::File,
    from: &Path,
    to_folder: &fs::File,
    to: &Path,
) -> Result<(), ErrorCode> {
    let beside = beside_name(to);
    let copied = copy_beside(from_folder, from, to_folder, &beside)
        .and_then(|()| beneath::rename(to_folder, &beside, to_folder, to));
    if let Err(error) = copied {
        // Nothing of a copy that failed stays beside the target.
        let _ = beneath::remove_file(to_folder, &beside);
        return Err(error.into());
    }
    Ok(beneath::remove_file(from_folder, from)?)
}

/// Copies the file at `from` beneath `from_folder` to a new file `beside` beneath `to_folder`,
/// written through to the disk.
fn copy_beside(
    from_folder: &fs::File,
    from: &Path,
    to_folder: &fs::File,
    beside: &Path,
) -> io::Result<()> {
    let mut reading = OpenOptions::new();
    reading.read(true);
    let mut writing = OpenOptions::new();
    writing.write(true).create_new(true);
    let mut source = beneath::open(from_folder, from, &reading)?;
    let mut copy = beneath::open(to_folder, beside, &writing)?;
    io::copy(&mut source, &mut copy)?;
    copy.sync_all()
}

/// A name for a copy beside the file `to`, hidden, of this process and its own.
fn beside_name(to: &Path) -> PathBuf {
    static COPIES: AtomicU64 = AtomicU64::new(0);
    let name = to.file_name().unwrap_or_default().to_string_lossy();
    let copy = COPIES.fetch_add(1, Ordering::Relaxed);
    let beside = format!(".{name}.waos-{}-{copy}", std::process::id());
    to.with_file_name(beside)
}

/// What a symlink on a machine holds, as the program names a path: an absolute one of this
/// computer by its name in the program ([`machine_paths`](super::machine_paths)), a relative one
/// as it is.
fn as_the_program_names(held: &Path) -> PathBuf {
    if held.is_absolute() {
        super::machine_paths::in_program(held).unwrap_or_else(|| held.to_path_buf())
    } else {
        held.to_path_buf()
    }
}

/// The folder of the machine holding what `path` names, opened as the machine resolves it, and
/// the name of what it holds there, which the change is made to beneath it.
fn pinned(path: &Path) -> Result<(Arc<fs::File>, PathBuf), ErrorCode> {
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        // The machine's own root, or a path ending in `..`, which names no entry to change.
        return Err(ErrorCode::Invalid);
    };
    Ok((Arc::new(open_folder(folder)?), PathBuf::from(name)))
}

/// Makes a symlink at `path` on a machine holding `contents`; on Windows a folder's where
/// `contents`, resolved from beside `path`, names a folder.
fn symlink_on_machine(contents: &str, path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(contents, path);
    #[cfg(windows)]
    {
        let beside = path.parent().unwrap_or(Path::new(""));
        if beside.join(contents).is_dir() {
            std::os::windows::fs::symlink_dir(contents, path)
        } else {
            std::os::windows::fs::symlink_file(contents, path)
        }
    }
}

/// Opens `path` on a machine as `request` asks, as the machine resolves it: a folder, or a file
/// unless a folder is asked for, the program's to change.
fn open_on_machine(path: &Path, request: &OpenRequest) -> Result<Descriptor, ErrorCode> {
    let handle = request.open_on_machine(path)?;
    let is_folder = handle.metadata()?.is_dir();
    let opened = Opened::on_machine(handle, path.to_path_buf(), request.open_mode());
    match (is_folder, request.directory) {
        (true, _) => Ok(Descriptor::Dir(opened)),
        (false, true) => Err(ErrorCode::NotDirectory),
        (false, false) => Ok(Descriptor::File(opened)),
    }
}

/// Opens `path` beneath `folder`, a writable mount's or not and of the kind given, as `request`
/// asks: a folder, or a file unless a folder is asked for.
fn open_beneath(
    folder: &fs::File,
    path: &Path,
    (writable, kind): (bool, MountKind),
    request: &OpenRequest,
) -> Result<Descriptor, ErrorCode> {
    if !writable && request.changes_the_file() {
        return Err(ErrorCode::ReadOnly);
    }
    let handle = beneath::open(folder, path, &request.options())?;
    let is_folder = handle.metadata()?.is_dir();
    let opened = Opened::new(handle, (writable, kind), request.open_mode());
    match (is_folder, request.directory) {
        (true, _) => Ok(Descriptor::Dir(opened)),
        (false, true) => Err(ErrorCode::NotDirectory),
        (false, false) => Ok(Descriptor::File(opened)),
    }
}

/// Whether a symlink the path ends in is followed, as [`beneath`] takes it.
fn following(follow: bool) -> FollowSymlinks {
    if follow {
        FollowSymlinks::Yes
    } else {
        FollowSymlinks::No
    }
}

impl OpenRequest {
    /// An open of a folder for reading what it holds.
    fn folder() -> OpenRequest {
        OpenRequest {
            follow: true,
            directory: true,
            read: true,
            ..OpenRequest::default()
        }
    }

    /// How the file is opened: for writing when asked to write, create or truncate; for reading
    /// when asked to, or when not asked to write, so that the system opens it and its mount
    /// decides what the program may do with it.
    pub(crate) fn open_mode(&self) -> OpenMode {
        OpenMode {
            read: self.read || !self.write,
            write: self.write || self.create || self.truncate,
        }
    }

    /// Opens `path` on the machine as this request asks, as the system resolves any path.
    #[cfg(unix)]
    fn open_on_machine(&self, path: &Path) -> io::Result<fs::File> {
        let opened = rustix::fs::open(path, self.flags(), Mode::from_bits_truncate(0o666))?;
        Ok(fs::File::from(opened))
    }

    /// Opens `path` on the machine as this request asks, as the system resolves any path: a
    /// folder too, which Windows opens only with `FILE_FLAG_BACKUP_SEMANTICS`.
    #[cfg(windows)]
    fn open_on_machine(&self, path: &Path) -> io::Result<fs::File> {
        use std::os::windows::fs::OpenOptionsExt as _;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        let mode = self.open_mode();
        let mut options = fs::OpenOptions::new();
        options
            .read(mode.read)
            .write(mode.write)
            .truncate(self.truncate);
        if self.create && self.exclusive {
            options.create_new(true);
        } else if self.create {
            options.create(true);
        }
        let mut flags = FILE_FLAG_BACKUP_SEMANTICS;
        if !self.follow {
            flags |= FILE_FLAG_OPEN_REPARSE_POINT;
        }
        options.custom_flags(flags).open(path)
    }

    /// The same request, as the flags of the system's `open`: none that keeps the path anywhere.
    #[cfg(unix)]
    pub(crate) fn flags(&self) -> OFlags {
        let mode = self.open_mode();
        let mut flags = match (mode.read, mode.write) {
            (true, true) => OFlags::RDWR,
            (false, true) => OFlags::WRONLY,
            (_, false) => OFlags::RDONLY,
        };
        flags |= OFlags::CLOEXEC;
        if self.create {
            flags |= OFlags::CREATE;
        }
        if self.create && self.exclusive {
            flags |= OFlags::EXCL;
        }
        if self.truncate {
            flags |= OFlags::TRUNC;
        }
        if !self.follow {
            flags |= OFlags::NOFOLLOW;
        }
        flags
    }

    /// The same request, as [`beneath`]'s options for its open.
    pub(crate) fn options(&self) -> OpenOptions {
        let mode = self.open_mode();
        let mut options = OpenOptions::new();
        options
            .read(mode.read)
            .write(mode.write)
            .truncate(self.truncate)
            .follow(following(self.follow));
        if self.create && self.exclusive {
            options.create_new(true);
        } else if self.create {
            options.create(true);
        }
        #[cfg(windows)]
        Self::open_folders_too(&mut options);
        options
    }

    /// Lets a Windows open open a folder, which it does only with `FILE_FLAG_BACKUP_SEMANTICS`, as
    /// wasmtime-wasi opens every path (its `filesystem::windows::maybe_dir`): with that flag, and
    /// shared for reading and writing, not deletion, so that no folder open beneath a mount is
    /// moved or removed under the paths walked from it.
    #[cfg(windows)]
    fn open_folders_too(options: &mut OpenOptions) {
        use super::beneath::OpenOptionsExt as _;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }

    /// Refuses what no open serves: synchronized I/O, and a folder made, emptied or required
    /// to be new.
    pub(super) fn check(&self) -> Result<(), ErrorCode> {
        if self.synchronized {
            return Err(ErrorCode::Unsupported);
        }
        if self.directory && (self.create || self.exclusive || self.truncate) {
            return Err(ErrorCode::Invalid);
        }
        Ok(())
    }

    /// Refuses what an open of a folder above the mounts cannot do, a folder that exists: be
    /// made new, written or emptied.
    fn check_folder_above(&self) -> Result<(), ErrorCode> {
        if self.create && self.exclusive {
            Err(ErrorCode::Exist)
        } else if self.changes_the_file() {
            Err(ErrorCode::IsDirectory)
        } else {
            Ok(())
        }
    }

    /// Whether the open would change the file: make it, empty it, or open it for writing.
    fn changes_the_file(&self) -> bool {
        self.create || self.open_mode().write
    }
}

#[cfg(test)]
mod tests {
    use super::super::descriptor::read_into;
    use super::super::folder_of_machine;
    use crate::access::ProgramAccess;

    fn any_access() -> ProgramAccess {
        ProgramAccess::new(Arc::new(crate::permissions::NothingGranted))
    }
    use super::*;

    /// The first bytes of what `file` holds, as a read from its start answers them.
    #[cfg(unix)]
    fn contents(file: &Opened) -> Vec<u8> {
        let mut buffer = vec![0; 100];
        let read = read_into(file.handle(), &mut buffer, 0).unwrap();
        buffer.truncate(read);
        buffer
    }

    #[cfg(unix)]
    #[test]
    fn a_machines_root_follows_an_absolute_symlink_and_dot_dot_above_it_where_a_mount_refuses() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = fs::canonicalize(scratch.path()).unwrap();
        fs::write(folder.join("note.txt"), "on the machine").unwrap();
        std::os::unix::fs::symlink(folder.join("note.txt"), folder.join("link")).unwrap();
        let Descriptor::Dir(root) = folder_of_machine(Path::new("/"), &any_access()).unwrap()
        else {
            panic!("a machine's root is a folder");
        };
        let from_root = folder.strip_prefix("/").unwrap();
        let read = OpenRequest {
            follow: true,
            read: true,
            ..OpenRequest::default()
        };
        for path in [
            from_root.join("link"),
            Path::new("../..").join(from_root).join("note.txt"),
        ] {
            let path = path.to_str().unwrap();
            let Descriptor::File(file) = root.beneath(path).open(&read).unwrap() else {
                panic!("{path} is a file");
            };
            assert_eq!(contents(&file), b"on the machine", "{path}");
        }

        let mount = Opened::new(
            fs::File::open(&folder).unwrap(),
            (true, MountKind::Granted),
            read.open_mode(),
        );
        let refused = mount.beneath("link").open(&read);
        assert!(refused.is_err(), "a mount keeps its path beneath it");
    }

    #[cfg(unix)]
    #[test]
    fn a_machines_root_changes_what_its_paths_name_wherever_they_lead() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = fs::canonicalize(scratch.path()).unwrap();
        let Descriptor::Dir(root) = folder_of_machine(Path::new("/"), &any_access()).unwrap()
        else {
            panic!("a machine's root is a folder");
        };
        let at = |name: &str| {
            let path = folder.join(name);
            root.beneath(path.to_str().unwrap())
        };
        at("made").create_directory().unwrap();
        at("made/inside").symlink("/").unwrap();
        at("made/inside").rename_to(&at("made/moved")).unwrap();
        assert_eq!(
            fs::read_link(folder.join("made/moved")).unwrap(),
            Path::new("/")
        );
        at("made/moved").unlink_file().unwrap();
        at("made").remove_directory().unwrap();
        assert!(!folder.join("made").exists());
        assert_eq!(root.beneath("").create_directory(), Err(ErrorCode::Invalid));
    }

    #[cfg(windows)]
    #[test]
    fn a_machines_root_holds_the_drives_and_a_drives_paths_by_its_letter() {
        assert!(matches!(
            folder_of_machine(Path::new("/"), &any_access()).unwrap(),
            Descriptor::Drives
        ));
        let scratch = tempfile::tempdir().unwrap();
        fs::write(scratch.path().join("note.txt"), "on the machine").unwrap();
        let named = super::super::machine_paths::in_program(scratch.path()).unwrap();
        let from_root = named.to_str().unwrap().trim_start_matches('/').to_owned();
        let read = OpenRequest {
            follow: true,
            read: true,
            ..OpenRequest::default()
        };
        for path in [
            format!("{from_root}/note.txt"),
            format!("../{from_root}/./note.txt"),
        ] {
            let Descriptor::File(file) = Target::in_drives(&path).open(&read).unwrap() else {
                panic!("{path} is a file");
            };
            assert_eq!(contents(&file), b"on the machine", "{path}");
        }

        let at = |name: &str| Target::in_drives(&format!("{from_root}/{name}"));
        at("made").create_directory().unwrap();
        at("made").rename_to(&at("moved")).unwrap();
        assert!(scratch.path().join("moved").is_dir());
        at("moved").remove_directory().unwrap();

        for drives in ["", ".", ".."] {
            assert!(
                matches!(Target::in_drives(drives), Target::Drives),
                "{drives}"
            );
        }
        assert!(matches!(
            Target::in_drives("not-a-drive"),
            Target::Outside {
                in_folder_above: true
            }
        ));
        assert_eq!(Target::Drives.create_directory(), Err(ErrorCode::Exist));
    }

    #[test]
    fn a_folder_beneath_a_mount_opens_as_a_folder_and_lists_what_it_holds() {
        let scratch = tempfile::tempdir().unwrap();
        fs::create_dir(scratch.path().join("notes")).unwrap();
        fs::write(scratch.path().join("notes/hello.txt"), "hello").unwrap();
        let folder = OpenRequest::folder();
        let mount = Opened::new(
            open_folder(scratch.path()).unwrap(),
            (true, MountKind::Granted),
            folder.open_mode(),
        );

        let Descriptor::Dir(notes) = mount.beneath("notes").open(&folder).unwrap() else {
            panic!("notes is a folder");
        };
        let names: Vec<String> = notes
            .read_directory()
            .unwrap()
            .into_iter()
            .map(|entry| entry.unwrap().name)
            .collect();
        assert_eq!(names, ["hello.txt"]);
    }

    #[test]
    fn a_replace_across_volumes_copies_beside_the_target_and_leaves_nothing_else() {
        let scratch = tempfile::tempdir().unwrap();
        let (from, to) = (scratch.path().join("cache"), scratch.path().join("picked"));
        fs::create_dir_all(&from).unwrap();
        fs::create_dir_all(&to).unwrap();
        fs::write(from.join("new.txt"), "second save").unwrap();
        fs::write(to.join("saved.txt"), "first save").unwrap();
        let (from_folder, to_folder) = (open_folder(&from).unwrap(), open_folder(&to).unwrap());

        replace_across_volumes(
            &from_folder,
            Path::new("new.txt"),
            &to_folder,
            Path::new("saved.txt"),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(to.join("saved.txt")).unwrap(),
            "second save"
        );
        assert_eq!(
            fs::read_dir(&to).unwrap().count(),
            1,
            "no copy left beside it"
        );
        assert_eq!(fs::read_dir(&from).unwrap().count(), 0, "moved, not copied");
    }
}
