//! What a file's status says, as WASI tells it, and the status of a folder above the mounts,
//! which the mount table makes up; a file's identity on its device, which tells two handles on
//! one file apart from two files and is what its metadata hash is of.
//!
//! On Windows a file's link count and identity are told of a handle, not in its metadata, so a
//! status is always read through a handle, one opened only for it where a path names the file, as
//! `wasmtime-wasi` reads it there; its status change time is its creation time, as `wasmtime-wasi`
//! gives it.
//!
//! The invariant: a file's status reads the same whichever call made it, an open handle's
//! `fstat`, or [`beneath`](super::beneath)'s `stat` by whichever route, so a hash taken through a
//! path equals the one taken through a handle on the same file.

use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::Path;
#[cfg(windows)]
use std::time::SystemTime;

#[cfg(unix)]
use super::beneath::{self, Metadata};
use super::beneath::{FileType, FollowSymlinks};
#[cfg(windows)]
use super::windows;

/// A file's status as the system tells it: what WASI's `descriptor-stat` says, and the file's
/// identity.
pub(crate) struct FileStat {
    pub(crate) status: DescriptorStat,
    pub(crate) identity: Identity,
}

/// What WASI's `descriptor-stat` says of a file or folder.
pub(crate) struct DescriptorStat {
    pub(crate) type_: DescriptorType,
    pub(crate) link_count: u64,
    /// A regular file's size in bytes, or the length of a symlink's contents.
    pub(crate) size: u64,
    pub(crate) data_access_timestamp: Option<Timestamp>,
    pub(crate) data_modification_timestamp: Option<Timestamp>,
    pub(crate) status_change_timestamp: Option<Timestamp>,
}

/// The type of what a descriptor or a folder's entry names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "Windows' files are folders, files and symlinks alone"
    )
)]
pub(crate) enum DescriptorType {
    Unknown,
    BlockDevice,
    CharacterDevice,
    Directory,
    Fifo,
    SymbolicLink,
    RegularFile,
    Socket,
}

/// A time a file keeps, from the Unix epoch, before it when the seconds are negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Timestamp {
    pub(crate) seconds: i64,
    pub(crate) nanoseconds: u32,
}

/// A file's device and inode, which no other file shares while it exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Identity {
    device: u64,
    inode: u64,
}

/// 128 bits of a hash of a file's identity, in two halves, as WASI has no 128-bit integer.
pub(crate) struct MetadataHashValue {
    pub(crate) lower: u64,
    pub(crate) upper: u64,
}

impl DescriptorStat {
    /// The status of a folder above the mounts: a folder of one link, empty to the system,
    /// whose times nothing keeps.
    pub(crate) fn folder_above() -> DescriptorStat {
        DescriptorStat {
            type_: DescriptorType::Directory,
            link_count: 1,
            size: 0,
            data_access_timestamp: None,
            data_modification_timestamp: None,
            status_change_timestamp: None,
        }
    }
}

impl FileStat {
    /// The status of the file or folder `file` is open on.
    #[cfg(unix)]
    pub(crate) fn of_file(file: &fs::File) -> io::Result<FileStat> {
        Ok(FileStat::from(&file.metadata()?))
    }

    /// The status of the file or folder `file` is open on: its metadata, and what Windows tells of
    /// its handle.
    #[cfg(windows)]
    pub(crate) fn of_file(file: &fs::File) -> io::Result<FileStat> {
        let metadata = file.metadata()?;
        let information = windows::handle_information(file)?;
        let time = |time: io::Result<SystemTime>| time.ok().and_then(Timestamp::of_system_time);
        Ok(FileStat {
            status: DescriptorStat {
                type_: DescriptorType::from(metadata.file_type()),
                link_count: information.link_count,
                size: metadata.len(),
                data_access_timestamp: time(metadata.accessed()),
                data_modification_timestamp: time(metadata.modified()),
                status_change_timestamp: time(metadata.created()),
            },
            identity: Identity {
                device: information.volume,
                inode: information.index,
            },
        })
    }

    /// The status of what `path` names beneath `folder`, or of the symlink it ends in unless
    /// `follow`.
    #[cfg(unix)]
    pub(crate) fn beneath(
        folder: &fs::File,
        path: &Path,
        follow: FollowSymlinks,
    ) -> io::Result<FileStat> {
        Ok(FileStat::from(&beneath::stat(folder, path, follow)?))
    }

    #[cfg(windows)]
    pub(crate) fn beneath(
        folder: &fs::File,
        path: &Path,
        follow: FollowSymlinks,
    ) -> io::Result<FileStat> {
        FileStat::of_file(&windows::status_handle_beneath(folder, path, follow)?)
    }

    /// The status of what `path` names on a machine, as the machine resolves it, or of the symlink
    /// it ends in unless `follow`.
    #[cfg(unix)]
    pub(crate) fn on_machine(path: &Path, follow: bool) -> io::Result<FileStat> {
        let metadata = if follow {
            fs::metadata(path)?
        } else {
            fs::symlink_metadata(path)?
        };
        Ok(FileStat::from(&metadata))
    }

    #[cfg(windows)]
    pub(crate) fn on_machine(path: &Path, follow: bool) -> io::Result<FileStat> {
        FileStat::of_file(&windows::status_handle_on_machine(path, follow)?)
    }
}

#[cfg(unix)]
impl From<&fs::Metadata> for FileStat {
    /// The status an open handle's `fstat` gives.
    fn from(metadata: &fs::Metadata) -> FileStat {
        FileStat {
            status: DescriptorStat {
                type_: DescriptorType::of_mode(metadata.mode()),
                link_count: metadata.nlink(),
                size: metadata.size(),
                data_access_timestamp: Timestamp::new(metadata.atime(), metadata.atime_nsec()),
                data_modification_timestamp: Timestamp::new(
                    metadata.mtime(),
                    metadata.mtime_nsec(),
                ),
                status_change_timestamp: Timestamp::new(metadata.ctime(), metadata.ctime_nsec()),
            },
            identity: Identity {
                device: metadata.dev(),
                inode: metadata.ino(),
            },
        }
    }
}

#[cfg(unix)]
impl From<&Metadata> for FileStat {
    /// The status [`beneath`](super::beneath)'s `stat` gives, from whichever call made it.
    fn from(metadata: &Metadata) -> FileStat {
        match metadata {
            Metadata::Std(metadata) => FileStat::from(metadata),
            Metadata::Stat(stat) => FileStat::from(stat),
            #[cfg(target_os = "linux")]
            Metadata::Statx(statx) => FileStat::from(statx),
        }
    }
}

#[cfg(unix)]
impl From<&rustix::fs::Stat> for FileStat {
    /// The status `fstatat` gives, its fields widened as std's `MetadataExt` widens them.
    #[allow(
        clippy::useless_conversion,
        clippy::unnecessary_cast,
        reason = "the fields' types differ by system, and are widened where they are narrower"
    )]
    fn from(stat: &rustix::fs::Stat) -> FileStat {
        FileStat {
            status: DescriptorStat {
                type_: DescriptorType::of_mode(u32::from(stat.st_mode)),
                link_count: u64::from(stat.st_nlink),
                size: stat.st_size as u64,
                data_access_timestamp: Timestamp::new(stat.st_atime, stat.st_atime_nsec),
                data_modification_timestamp: Timestamp::new(stat.st_mtime, stat.st_mtime_nsec),
                status_change_timestamp: Timestamp::new(stat.st_ctime, stat.st_ctime_nsec),
            },
            identity: Identity {
                device: stat.st_dev as u64,
                inode: stat.st_ino,
            },
        }
    }
}

#[cfg(target_os = "linux")]
impl From<&rustix::fs::Statx> for FileStat {
    /// The status Linux's `statx` gives, each time where the system filled it in, the device made
    /// one number as std's `MetadataExt` makes it.
    fn from(statx: &rustix::fs::Statx) -> FileStat {
        use rustix::fs::{StatxFlags, StatxTimestamp};
        let filled = StatxFlags::from_bits_retain(statx.stx_mask);
        let time = |flag: StatxFlags, time: &StatxTimestamp| {
            filled
                .contains(flag)
                .then(|| Timestamp::new(time.tv_sec, time.tv_nsec))
                .flatten()
        };
        FileStat {
            status: DescriptorStat {
                type_: DescriptorType::of_mode(u32::from(statx.stx_mode)),
                link_count: u64::from(statx.stx_nlink),
                size: statx.stx_size,
                data_access_timestamp: time(StatxFlags::ATIME, &statx.stx_atime),
                data_modification_timestamp: time(StatxFlags::MTIME, &statx.stx_mtime),
                status_change_timestamp: time(StatxFlags::CTIME, &statx.stx_ctime),
            },
            identity: Identity {
                device: rustix::fs::makedev(statx.stx_dev_major, statx.stx_dev_minor),
                inode: statx.stx_ino,
            },
        }
    }
}

#[cfg(unix)]
impl DescriptorType {
    /// The type a status's mode says, by the bits POSIX gives each type.
    fn of_mode(mode: u32) -> DescriptorType {
        match mode & 0o170_000 {
            0o040_000 => DescriptorType::Directory,
            0o100_000 => DescriptorType::RegularFile,
            0o120_000 => DescriptorType::SymbolicLink,
            0o060_000 => DescriptorType::BlockDevice,
            0o020_000 => DescriptorType::CharacterDevice,
            0o010_000 => DescriptorType::Fifo,
            0o140_000 => DescriptorType::Socket,
            _ => DescriptorType::Unknown,
        }
    }
}

impl From<FileType> for DescriptorType {
    /// The type a folder's entry has, as [`beneath`](super::beneath)'s `read_dir` reads it: the
    /// system's word for it, or std's.
    fn from(file_type: FileType) -> DescriptorType {
        match file_type {
            FileType::Std(file_type) => DescriptorType::from(file_type),
            #[cfg(unix)]
            FileType::Unix(file_type) => DescriptorType::from(file_type),
        }
    }
}

#[cfg(unix)]
impl From<rustix::fs::FileType> for DescriptorType {
    /// The type of a folder's entry as the system lists it, unknown where the system does not say.
    fn from(file_type: rustix::fs::FileType) -> DescriptorType {
        use rustix::fs::FileType as Unix;
        match file_type {
            Unix::RegularFile => DescriptorType::RegularFile,
            Unix::Directory => DescriptorType::Directory,
            Unix::Symlink => DescriptorType::SymbolicLink,
            Unix::Fifo => DescriptorType::Fifo,
            Unix::Socket => DescriptorType::Socket,
            Unix::CharacterDevice => DescriptorType::CharacterDevice,
            Unix::BlockDevice => DescriptorType::BlockDevice,
            Unix::Unknown => DescriptorType::Unknown,
        }
    }
}

impl From<fs::FileType> for DescriptorType {
    /// The type of what std's metadata is of: on Windows a folder, a file or a symlink, or unknown.
    fn from(file_type: fs::FileType) -> DescriptorType {
        if file_type.is_dir() {
            DescriptorType::Directory
        } else if file_type.is_file() {
            DescriptorType::RegularFile
        } else if file_type.is_symlink() {
            DescriptorType::SymbolicLink
        } else {
            DescriptorType::of_unix_file_type(file_type)
        }
    }
}

impl DescriptorType {
    /// The types only Unix's files have, of a file neither a folder, a file nor a symlink.
    #[cfg(unix)]
    fn of_unix_file_type(file_type: fs::FileType) -> DescriptorType {
        if file_type.is_block_device() {
            DescriptorType::BlockDevice
        } else if file_type.is_char_device() {
            DescriptorType::CharacterDevice
        } else if file_type.is_fifo() {
            DescriptorType::Fifo
        } else if file_type.is_socket() {
            DescriptorType::Socket
        } else {
            DescriptorType::Unknown
        }
    }

    #[cfg(windows)]
    fn of_unix_file_type(_file_type: fs::FileType) -> DescriptorType {
        DescriptorType::Unknown
    }
}

impl Timestamp {
    /// `time`, from the epoch; none before it beyond what an `i64` of seconds holds.
    #[cfg(windows)]
    fn of_system_time(time: SystemTime) -> Option<Timestamp> {
        match time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(after) => Some(Timestamp {
                seconds: i64::try_from(after.as_secs()).ok()?,
                nanoseconds: after.subsec_nanos(),
            }),
            Err(before) => {
                let before = before.duration();
                let seconds = i64::try_from(before.as_secs()).ok()?;
                match before.subsec_nanos() {
                    0 => Some(Timestamp {
                        seconds: -seconds,
                        nanoseconds: 0,
                    }),
                    nanoseconds => Some(Timestamp {
                        seconds: -seconds - 1,
                        nanoseconds: 1_000_000_000 - nanoseconds,
                    }),
                }
            }
        }
    }

    /// The time `seconds` and `nanoseconds` from the epoch, as a status gives them, of whichever
    /// type the system gives the nanoseconds in; none where they are out of a second's range, as
    /// no system gives them.
    #[cfg(unix)]
    fn new(seconds: i64, nanoseconds: impl TryInto<u32>) -> Option<Timestamp> {
        let nanoseconds = nanoseconds.try_into().ok()?;
        Some(Timestamp {
            seconds,
            nanoseconds,
        })
    }
}

impl MetadataHashValue {
    /// The hash of a file's identity.
    pub(crate) fn of_file(identity: Identity) -> MetadataHashValue {
        MetadataHashValue::of(identity)
    }

    /// The hash of a folder above the mounts, which has no identity on the machine: of its path.
    pub(crate) fn of_folder_above(path: &Path) -> MetadataHashValue {
        MetadataHashValue::of(path)
    }

    /// 64 bits of hash from std's hasher, which hashes alike in every run of the same build, and
    /// the upper half those bits made to differ from the lower by an arbitrary constant, the
    /// bits of the double nearest pi, so that no reader takes the halves for a mangled copy.
    fn of(value: impl Hash) -> MetadataHashValue {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        let lower = hasher.finish();
        MetadataHashValue {
            lower,
            upper: lower ^ 4_614_256_656_552_045_848,
        }
    }
}
