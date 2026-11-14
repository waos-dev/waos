//! WASI 0.2's types for the files' own, each way a call needs.

use std::time::{Duration, SystemTime};

use wasmtime_wasi::p2::bindings::clocks::wall_clock::Datetime;

use super::bindings::wasi::filesystem::types;
use crate::files::descriptor::{Advice, DescriptorFlags, DirectoryEntry};
use crate::files::errors::ErrorCode;
use crate::files::status::{DescriptorStat, DescriptorType, MetadataHashValue, Timestamp};
use crate::files::target::OpenRequest;

/// The open a program's `open-at` asks for.
pub(super) fn open_request(
    path_flags: types::PathFlags,
    open_flags: types::OpenFlags,
    flags: types::DescriptorFlags,
) -> OpenRequest {
    OpenRequest {
        follow: follows(path_flags),
        create: open_flags.contains(types::OpenFlags::CREATE),
        exclusive: open_flags.contains(types::OpenFlags::EXCLUSIVE),
        truncate: open_flags.contains(types::OpenFlags::TRUNCATE),
        directory: open_flags.contains(types::OpenFlags::DIRECTORY),
        read: flags.contains(types::DescriptorFlags::READ),
        write: flags.contains(types::DescriptorFlags::WRITE),
        synchronized: flags.intersects(
            types::DescriptorFlags::FILE_INTEGRITY_SYNC
                | types::DescriptorFlags::DATA_INTEGRITY_SYNC
                | types::DescriptorFlags::REQUESTED_WRITE_SYNC,
        ),
    }
}

/// Whether a symlink the path ends in is followed.
pub(super) fn follows(path_flags: types::PathFlags) -> bool {
    path_flags.contains(types::PathFlags::SYMLINK_FOLLOW)
}

/// The time to set, or none to leave it.
pub(super) fn time_to_set(timestamp: types::NewTimestamp) -> Result<Option<SystemTime>, ErrorCode> {
    match timestamp {
        types::NewTimestamp::NoChange => Ok(None),
        types::NewTimestamp::Now => Ok(Some(SystemTime::now())),
        types::NewTimestamp::Timestamp(time) => system_time(time).map(Some),
    }
}

/// `time` as the system takes it; `overflow` past what it holds.
fn system_time(time: Datetime) -> Result<SystemTime, ErrorCode> {
    let seconds = time
        .seconds
        .checked_add(u64::from(time.nanoseconds / 1_000_000_000))
        .ok_or(ErrorCode::Overflow)?;
    let since_epoch = Duration::new(seconds, time.nanoseconds % 1_000_000_000);
    SystemTime::UNIX_EPOCH
        .checked_add(since_epoch)
        .ok_or(ErrorCode::Overflow)
}

impl From<ErrorCode> for types::ErrorCode {
    fn from(code: ErrorCode) -> types::ErrorCode {
        match code {
            ErrorCode::Access => types::ErrorCode::Access,
            ErrorCode::Already => types::ErrorCode::Already,
            ErrorCode::BadDescriptor => types::ErrorCode::BadDescriptor,
            ErrorCode::Busy => types::ErrorCode::Busy,
            ErrorCode::CrossDevice => types::ErrorCode::CrossDevice,
            ErrorCode::Exist => types::ErrorCode::Exist,
            ErrorCode::FileTooLarge => types::ErrorCode::FileTooLarge,
            ErrorCode::IllegalByteSequence => types::ErrorCode::IllegalByteSequence,
            ErrorCode::InProgress => types::ErrorCode::InProgress,
            ErrorCode::Interrupted => types::ErrorCode::Interrupted,
            ErrorCode::Invalid => types::ErrorCode::Invalid,
            ErrorCode::Io => types::ErrorCode::Io,
            ErrorCode::IsDirectory => types::ErrorCode::IsDirectory,
            ErrorCode::Loop => types::ErrorCode::Loop,
            ErrorCode::TooManyLinks => types::ErrorCode::TooManyLinks,
            ErrorCode::NameTooLong => types::ErrorCode::NameTooLong,
            ErrorCode::NoEntry => types::ErrorCode::NoEntry,
            ErrorCode::InsufficientMemory => types::ErrorCode::InsufficientMemory,
            ErrorCode::InsufficientSpace => types::ErrorCode::InsufficientSpace,
            ErrorCode::NotDirectory => types::ErrorCode::NotDirectory,
            ErrorCode::NotEmpty => types::ErrorCode::NotEmpty,
            ErrorCode::Unsupported => types::ErrorCode::Unsupported,
            ErrorCode::Overflow => types::ErrorCode::Overflow,
            ErrorCode::NotPermitted => types::ErrorCode::NotPermitted,
            ErrorCode::Pipe => types::ErrorCode::Pipe,
            ErrorCode::ReadOnly => types::ErrorCode::ReadOnly,
            ErrorCode::InvalidSeek => types::ErrorCode::InvalidSeek,
        }
    }
}

impl From<DescriptorType> for types::DescriptorType {
    fn from(type_: DescriptorType) -> types::DescriptorType {
        match type_ {
            DescriptorType::Unknown => types::DescriptorType::Unknown,
            DescriptorType::BlockDevice => types::DescriptorType::BlockDevice,
            DescriptorType::CharacterDevice => types::DescriptorType::CharacterDevice,
            DescriptorType::Directory => types::DescriptorType::Directory,
            DescriptorType::Fifo => types::DescriptorType::Fifo,
            DescriptorType::SymbolicLink => types::DescriptorType::SymbolicLink,
            DescriptorType::RegularFile => types::DescriptorType::RegularFile,
            DescriptorType::Socket => types::DescriptorType::Socket,
        }
    }
}

impl From<DescriptorFlags> for types::DescriptorFlags {
    fn from(flags: DescriptorFlags) -> types::DescriptorFlags {
        [
            (flags.read, types::DescriptorFlags::READ),
            (flags.write, types::DescriptorFlags::WRITE),
            (
                flags.mutate_directory,
                types::DescriptorFlags::MUTATE_DIRECTORY,
            ),
        ]
        .into_iter()
        .filter(|(set, _)| *set)
        .fold(types::DescriptorFlags::empty(), |wasi, (_, flag)| {
            wasi | flag
        })
    }
}

impl From<DescriptorStat> for types::DescriptorStat {
    /// A time before the epoch, which WASI 0.2's clock cannot say, is said to be unkept.
    fn from(status: DescriptorStat) -> types::DescriptorStat {
        types::DescriptorStat {
            type_: status.type_.into(),
            link_count: status.link_count,
            size: status.size,
            data_access_timestamp: status.data_access_timestamp.and_then(datetime),
            data_modification_timestamp: status.data_modification_timestamp.and_then(datetime),
            status_change_timestamp: status.status_change_timestamp.and_then(datetime),
        }
    }
}

/// `time` on WASI 0.2's wall clock, which starts at the epoch.
fn datetime(time: Timestamp) -> Option<Datetime> {
    Some(Datetime {
        seconds: u64::try_from(time.seconds).ok()?,
        nanoseconds: time.nanoseconds,
    })
}

impl From<MetadataHashValue> for types::MetadataHashValue {
    fn from(hash: MetadataHashValue) -> types::MetadataHashValue {
        types::MetadataHashValue {
            lower: hash.lower,
            upper: hash.upper,
        }
    }
}

impl From<DirectoryEntry> for types::DirectoryEntry {
    fn from(entry: DirectoryEntry) -> types::DirectoryEntry {
        types::DirectoryEntry {
            type_: entry.type_.into(),
            name: entry.name,
        }
    }
}

impl From<types::Advice> for Advice {
    fn from(advice: types::Advice) -> Advice {
        match advice {
            types::Advice::Normal => Advice::Normal,
            types::Advice::Sequential => Advice::Sequential,
            types::Advice::Random => Advice::Random,
            types::Advice::WillNeed => Advice::WillNeed,
            types::Advice::DontNeed => Advice::DontNeed,
            types::Advice::NoReuse => Advice::NoReuse,
        }
    }
}
