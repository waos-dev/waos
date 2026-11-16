//! A program's open handles on its files, and the calls made on a handle itself, without a path:
//! a folder above the mounts, named by its path; or a file or folder of the machine, opened
//! beneath a mount, with what that mount allowed, or from a machine's root, with where it is.
//!
//! The invariant: a handle keeps what its mount allowed when it was opened, so it stays valid,
//! with the same rights, after its mount is removed or changed; and it is told written once, at
//! its first write, whichever copy of it writes.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use super::beneath;
use super::errors::ErrorCode;
use super::mounts::{MountOf, Mounted};
use super::status::{DescriptorType, FileStat};
use super::target::{Beneath, Target};
use crate::access::ActiveUse;

/// The most one read takes at once, what `wasmtime-wasi` allocates for one.
pub(crate) const READ_LIMIT: usize = 64 * 1024;

/// A program's handle on a folder or a file. Public, as the bindings that name it as
/// `wasi:filesystem`'s `descriptor` are; the crate does not export it.
pub enum Descriptor {
    /// A folder above the mounts, named by its path in the program, which every call resolves
    /// through the mount table as it is then.
    Above(PathBuf),
    Dir(Opened),
    File(Opened),
    /// On Windows, this computer's root as a machine: the folder of its drives, each by its letter
    /// ([`machine_paths`](super::machine_paths)), which nothing changes.
    #[cfg(windows)]
    Drives,
}

/// A file or folder of the machine, opened beneath a mount, or from a machine's root.
#[derive(Clone)]
pub struct Opened {
    handle: Arc<fs::File>,
    /// The mount it was opened in, as it was then: whether it let the program change it, among
    /// what it keeps.
    mount: MountOf,
    open_mode: OpenMode,
    /// Whether it has been written through, shared by every copy of the handle.
    written: Arc<AtomicBool>,
    /// Where it is on the machine, opened from a machine's root, whose paths resolve as the
    /// system resolves any path; none beneath a mount, whose paths resolve beneath it.
    on_machine: Option<PathBuf>,
    /// The program's use of it as an open file, shared by every copy of the handle and each
    /// stream on it, so it ends as the last goes; none for a mount's own folder.
    open_use: Option<Arc<ActiveUse>>,
}

/// An open file's handle as a stream reads or writes through it, with what keeps it among the
/// program's open files while the stream lasts.
pub(crate) struct StreamedFile {
    pub(crate) file: Arc<fs::File>,
    pub(crate) open_use: Option<Arc<ActiveUse>>,
}

/// What a file or folder was opened for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OpenMode {
    pub(crate) read: bool,
    pub(crate) write: bool,
}

/// What a handle may do, as WASI's descriptor flags say it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DescriptorFlags {
    pub(crate) read: bool,
    pub(crate) write: bool,
    /// A folder's: whether what it holds may be changed through it.
    pub(crate) mutate_directory: bool,
}

/// What a program expects of how it will read a range of a file.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Advice {
    Normal,
    Sequential,
    Random,
    WillNeed,
    DontNeed,
    NoReuse,
}

/// An entry of a folder: its name and its type.
pub(crate) struct DirectoryEntry {
    pub(crate) type_: DescriptorType,
    pub(crate) name: String,
}

impl DirectoryEntry {
    /// The entry a folder above the mounts holds for `name`: a folder.
    pub(crate) fn folder_above(name: OsString) -> Result<DirectoryEntry, ErrorCode> {
        DirectoryEntry::above(name, DescriptorType::Directory)
    }

    /// The entry a folder above the mounts holds for `name`, a file mounted there.
    pub(crate) fn file_above(name: OsString) -> Result<DirectoryEntry, ErrorCode> {
        DirectoryEntry::above(name, DescriptorType::RegularFile)
    }

    fn above(name: OsString, type_: DescriptorType) -> Result<DirectoryEntry, ErrorCode> {
        let name = name
            .into_string()
            .map_err(|_| ErrorCode::IllegalByteSequence)?;
        Ok(DirectoryEntry { type_, name })
    }
}

impl Descriptor {
    /// The file this handle is on, for a call only a file takes.
    pub(crate) fn file(&self) -> Result<&Opened, ErrorCode> {
        match self {
            Descriptor::File(file) => Ok(file),
            Descriptor::Dir(_) | Descriptor::Above(_) => Err(ErrorCode::BadDescriptor),
            #[cfg(windows)]
            Descriptor::Drives => Err(ErrorCode::BadDescriptor),
        }
    }

    pub(crate) fn flags(&self) -> DescriptorFlags {
        match self {
            Descriptor::Above(_) => DescriptorFlags {
                read: true,
                ..DescriptorFlags::default()
            },
            #[cfg(windows)]
            Descriptor::Drives => DescriptorFlags {
                read: true,
                ..DescriptorFlags::default()
            },
            Descriptor::Dir(folder) => DescriptorFlags {
                read: folder.open_mode.read,
                mutate_directory: folder.open_mode.write,
                ..DescriptorFlags::default()
            },
            Descriptor::File(file) => DescriptorFlags {
                read: file.open_mode.read,
                write: file.open_mode.write,
                ..DescriptorFlags::default()
            },
        }
    }

    pub(crate) fn type_(&self) -> Result<DescriptorType, ErrorCode> {
        match self {
            Descriptor::File(file) => Ok(file.file_status()?.status.type_),
            Descriptor::Dir(_) | Descriptor::Above(_) => Ok(DescriptorType::Directory),
            #[cfg(windows)]
            Descriptor::Drives => Ok(DescriptorType::Directory),
        }
    }

    /// Sets the handle's times; a time of none is left as it is.
    pub(crate) fn set_times(
        &self,
        accessed: Option<SystemTime>,
        modified: Option<SystemTime>,
    ) -> Result<(), ErrorCode> {
        match self {
            Descriptor::Above(_) => Err(ErrorCode::ReadOnly),
            #[cfg(windows)]
            Descriptor::Drives => Err(ErrorCode::ReadOnly),
            Descriptor::Dir(opened) | Descriptor::File(opened) => {
                opened.set_times(accessed, modified)
            }
        }
    }

    /// Waits until what was written through the handle, and its metadata, is on the disk.
    pub(crate) fn sync(&self) -> Result<(), ErrorCode> {
        match self {
            Descriptor::Above(_) => Ok(()),
            #[cfg(windows)]
            Descriptor::Drives => Ok(()),
            Descriptor::Dir(opened) | Descriptor::File(opened) => Ok(opened.handle.sync_all()?),
        }
    }

    /// Waits until what was written through the handle is on the disk.
    pub(crate) fn sync_data(&self) -> Result<(), ErrorCode> {
        match self {
            Descriptor::Above(_) => Ok(()),
            #[cfg(windows)]
            Descriptor::Drives => Ok(()),
            Descriptor::Dir(opened) | Descriptor::File(opened) => Ok(opened.handle.sync_data()?),
        }
    }

    /// Whether the two handles are on the same file or folder.
    pub(crate) fn is_same_object(&self, other: &Descriptor) -> Result<bool, ErrorCode> {
        match (self, other) {
            (Descriptor::Above(path), Descriptor::Above(other)) => Ok(path == other),
            #[cfg(windows)]
            (Descriptor::Drives, Descriptor::Drives) => Ok(true),
            (
                Descriptor::Dir(opened) | Descriptor::File(opened),
                Descriptor::Dir(other) | Descriptor::File(other),
            ) => Ok(opened.file_status()?.identity == other.file_status()?.identity),
            _ => Ok(false),
        }
    }
}

impl Opened {
    /// `handle`, opened as `open_mode` says in `mount`.
    pub(crate) fn new(handle: fs::File, mount: MountOf, open_mode: OpenMode) -> Opened {
        Opened {
            handle: Arc::new(handle),
            mount,
            open_mode,
            written: Arc::default(),
            on_machine: None,
            open_use: None,
        }
    }

    /// The handle, kept among the program's open files by `open_use` from now on.
    pub(crate) fn used_as(mut self, open_use: ActiveUse) -> Opened {
        self.open_use = Some(Arc::new(open_use));
        self
    }

    /// The handle, for a stream to read or write through, with what keeps it open.
    pub(crate) fn streamed(&self) -> StreamedFile {
        StreamedFile {
            file: Arc::clone(&self.handle),
            open_use: self.open_use.clone(),
        }
    }

    /// `handle`, at `at` on a machine the user gave the program whole: writable, its paths
    /// resolving as the system resolves any path.
    pub(crate) fn on_machine(handle: fs::File, at: PathBuf, open_mode: OpenMode) -> Opened {
        Opened {
            on_machine: Some(at),
            ..Opened::new(handle, MountOf::machine(), open_mode)
        }
    }

    /// A mount's own folder, as opened for the table.
    pub(crate) fn of_mount(mount: &Mounted) -> Opened {
        Opened {
            handle: Arc::clone(&mount.folder),
            mount: mount.of(),
            open_mode: OpenMode {
                read: true,
                write: false,
            },
            written: Arc::default(),
            on_machine: None,
            open_use: None,
        }
    }

    /// `path` from this folder: beneath it, inside the mount it was opened in; or, from a
    /// machine's root, wherever the machine resolves it.
    pub(crate) fn beneath(&self, path: &str) -> Target {
        if let Some(at) = &self.on_machine {
            return Target::OnMachine(at.join(path.trim_start_matches('/')));
        }
        Target::Beneath(Beneath {
            folder: Arc::clone(&self.handle),
            path: PathBuf::from(path),
            mount: self.mount.clone(),
            mounted_file: false,
        })
    }

    /// Whether this was opened from a machine's root, whose paths the machine resolves.
    pub(crate) fn is_on_machine(&self) -> bool {
        self.on_machine.is_some()
    }

    /// The system's handle, which a stream reads or writes through.
    pub(crate) fn handle(&self) -> &Arc<fs::File> {
        &self.handle
    }

    /// The mount it was opened in.
    pub(crate) fn mount(&self) -> &MountOf {
        &self.mount
    }

    pub(crate) fn open_mode(&self) -> OpenMode {
        self.open_mode
    }

    /// Whether this is the first write through the handle, and from now on it is written.
    pub(crate) fn first_write(&self) -> bool {
        !self.written.swap(true, Ordering::Relaxed)
    }

    /// Refuses a write where the file's mount is read-only.
    pub(crate) fn check_writable(&self) -> Result<(), ErrorCode> {
        if self.mount.writable {
            Ok(())
        } else {
            Err(ErrorCode::ReadOnly)
        }
    }

    /// The status the system gives of the handle's file or folder.
    pub(crate) fn file_status(&self) -> Result<FileStat, ErrorCode> {
        Ok(FileStat::of_file(&self.handle)?)
    }

    pub(crate) fn set_times(
        &self,
        accessed: Option<SystemTime>,
        modified: Option<SystemTime>,
    ) -> Result<(), ErrorCode> {
        self.check_writable()?;
        Ok(beneath::set_times_on_fd(&self.handle, accessed, modified)?)
    }

    pub(crate) fn set_size(&self, size: u64) -> Result<(), ErrorCode> {
        self.check_writable()?;
        Ok(self.handle.set_len(size)?)
    }

    pub(crate) fn advise(&self, offset: u64, len: u64, advice: Advice) -> Result<(), ErrorCode> {
        Ok(advise(&self.handle, offset, len, advice)?)
    }

    /// The entries of this folder, each read as the system lists it; an entry that fails to read
    /// is its error, told when the program reaches it.
    pub(crate) fn read_directory(
        &self,
    ) -> Result<Vec<Result<DirectoryEntry, ErrorCode>>, ErrorCode> {
        let entries = beneath::read_dir(&self.handle)?;
        Ok(entries
            .map(|entry| {
                let (name, file_type) = entry?;
                let type_ = DescriptorType::from(file_type);
                let name = name
                    .into_string()
                    .map_err(|_| ErrorCode::IllegalByteSequence)?;
                Ok(DirectoryEntry { type_, name })
            })
            .collect())
    }
}

/// Reads into `buffer` from `offset` in `file`, again when a signal interrupts the read, and
/// answers how many bytes it read: none at the file's end.
pub(crate) fn read_into(file: &fs::File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    loop {
        match read_into_at(file, buffer, offset) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

/// Reads into `buffer` from `offset` in `file`. On Windows the file's own position moves to after
/// what was read, which nothing reads by: every read and write here names its offset.
fn read_into_at(file: &fs::File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    #[cfg(unix)]
    return std::os::unix::fs::FileExt::read_at(file, buffer, offset);
    #[cfg(windows)]
    return std::os::windows::fs::FileExt::seek_read(file, buffer, offset);
}

/// Writes `bytes` at `offset` in `file`, and answers how many were written; on Windows the file's
/// own position moves, as for a read.
pub(crate) fn write_at(file: &fs::File, bytes: &[u8], offset: u64) -> io::Result<usize> {
    #[cfg(unix)]
    return std::os::unix::fs::FileExt::write_at(file, bytes, offset);
    #[cfg(windows)]
    return std::os::windows::fs::FileExt::seek_write(file, bytes, offset);
}

/// Writes `bytes` at the end of `file`, wherever its end is when the write is made, and answers
/// how many were written. Linux writes there in one call; Windows through a handle reopened to
/// append (`files::windows`); elsewhere the open file is switched to appending for the write and
/// back, which is not atomic for whoever else shares the open file: nothing does while the program
/// writes, as its calls are made on its one thread.
#[cfg(windows)]
pub(crate) fn append(file: &fs::File, bytes: &[u8]) -> io::Result<usize> {
    super::windows::append(file, bytes)
}

#[cfg(unix)]
pub(crate) fn append(file: &fs::File, bytes: &[u8]) -> io::Result<usize> {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};

    #[cfg(target_os = "linux")]
    {
        use rustix::io::{Errno, ReadWriteFlags, pwritev2};
        match pwritev2(file, &[io::IoSlice::new(bytes)], 0, ReadWriteFlags::APPEND) {
            Err(Errno::NOSYS | Errno::NOTSUP) => {}
            written => return Ok(written?),
        }
    }
    let flags = fcntl_getfl(file)?;
    fcntl_setfl(file, flags | OFlags::APPEND)?;
    let written = rustix::io::write(file, bytes);
    fcntl_setfl(file, flags)?;
    Ok(written?)
}

/// Tells the system how the range will be read, where it takes advice: Apple's systems read
/// ahead what will be needed, Linux takes every advice, and elsewhere none is taken.
fn advise(file: &fs::File, offset: u64, len: u64, advice: Advice) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    if let Advice::WillNeed = advice {
        rustix::fs::fcntl_rdadvise(file, offset, len)?;
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let advice = match advice {
            Advice::Normal => rustix::fs::Advice::Normal,
            Advice::Sequential => rustix::fs::Advice::Sequential,
            Advice::Random => rustix::fs::Advice::Random,
            Advice::WillNeed => rustix::fs::Advice::WillNeed,
            Advice::DontNeed => rustix::fs::Advice::DontNeed,
            Advice::NoReuse => rustix::fs::Advice::NoReuse,
        };
        rustix::fs::fadvise(file, offset, std::num::NonZeroU64::new(len), advice)?;
    }
    #[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
    let _ = (file, offset, len, advice);
    Ok(())
}
