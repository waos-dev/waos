//! Why a file call failed, as WASI names it, and what the system's errors become.
//!
//! The invariant: a path refused for leaving its mount is `not-permitted`, whichever route
//! refused it, as `wasmtime-wasi` refuses one leaving a preopened folder: every route of
//! [`beneath`](super::beneath) refuses one with `PermissionDenied` and no system error number.

use std::io;

#[cfg(unix)]
use rustix::io::Errno;
use tracing::debug;

/// Why a file call failed: `wasi:filesystem`'s `error-code`, in the cases the engine gives, each
/// named after the `errno` it stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "Windows' errors name fewer of WASI's codes than Unix's"
    )
)]
pub(crate) enum ErrorCode {
    /// `EACCES`
    Access,
    /// `EALREADY`
    Already,
    /// `EBADF`
    BadDescriptor,
    /// `EBUSY`
    Busy,
    /// `EXDEV`
    CrossDevice,
    /// `EEXIST`
    Exist,
    /// `EFBIG`
    FileTooLarge,
    /// `EILSEQ`
    IllegalByteSequence,
    /// `EINPROGRESS`
    InProgress,
    /// `EINTR`
    Interrupted,
    /// `EINVAL`
    Invalid,
    /// `EIO`
    Io,
    /// `EISDIR`
    IsDirectory,
    /// `ELOOP`
    Loop,
    /// `EMLINK`
    TooManyLinks,
    /// `ENAMETOOLONG`
    NameTooLong,
    /// `ENOENT`
    NoEntry,
    /// `ENOMEM`
    InsufficientMemory,
    /// `ENOSPC`
    InsufficientSpace,
    /// `ENOTDIR`
    NotDirectory,
    /// `ENOTEMPTY`
    NotEmpty,
    /// `ENOTSUP` and `ENOSYS`
    Unsupported,
    /// `EOVERFLOW`
    Overflow,
    /// `EPERM`
    NotPermitted,
    /// `EPIPE`
    Pipe,
    /// `EROFS`
    ReadOnly,
    /// `ESPIPE`
    InvalidSeek,
}

impl From<&io::Error> for ErrorCode {
    /// The system's error by its number, or by its kind when it has none or one WASI does not
    /// name.
    fn from(error: &io::Error) -> ErrorCode {
        match error.raw_os_error().and_then(of_errno) {
            Some(code) => code,
            None => {
                debug!(%error, "a file call's error WASI has no number for");
                of_kind(error.kind())
            }
        }
    }
}

impl From<io::Error> for ErrorCode {
    fn from(error: io::Error) -> ErrorCode {
        ErrorCode::from(&error)
    }
}

/// The code for the system's error number `raw`, where WASI names one.
#[cfg(unix)]
fn of_errno(raw: i32) -> Option<ErrorCode> {
    Some(match Errno::from_raw_os_error(raw) {
        Errno::PIPE => ErrorCode::Pipe,
        Errno::PERM => ErrorCode::NotPermitted,
        Errno::NOENT => ErrorCode::NoEntry,
        Errno::NOMEM => ErrorCode::InsufficientMemory,
        Errno::IO => ErrorCode::Io,
        Errno::BADF => ErrorCode::BadDescriptor,
        Errno::BUSY => ErrorCode::Busy,
        Errno::ACCESS => ErrorCode::Access,
        Errno::NOTDIR => ErrorCode::NotDirectory,
        Errno::ISDIR => ErrorCode::IsDirectory,
        Errno::INVAL => ErrorCode::Invalid,
        Errno::EXIST => ErrorCode::Exist,
        Errno::FBIG => ErrorCode::FileTooLarge,
        Errno::NOSPC => ErrorCode::InsufficientSpace,
        Errno::SPIPE => ErrorCode::InvalidSeek,
        Errno::MLINK => ErrorCode::TooManyLinks,
        Errno::NAMETOOLONG => ErrorCode::NameTooLong,
        Errno::NOTEMPTY => ErrorCode::NotEmpty,
        Errno::LOOP => ErrorCode::Loop,
        Errno::OVERFLOW => ErrorCode::Overflow,
        Errno::ILSEQ => ErrorCode::IllegalByteSequence,
        Errno::NOTSUP | Errno::NOSYS => ErrorCode::Unsupported,
        Errno::ALREADY => ErrorCode::Already,
        Errno::INPROGRESS => ErrorCode::InProgress,
        Errno::INTR => ErrorCode::Interrupted,
        Errno::ROFS => ErrorCode::ReadOnly,
        Errno::XDEV => ErrorCode::CrossDevice,
        #[allow(
            unreachable_patterns,
            reason = "on some systems `EOPNOTSUPP` is `ENOTSUP`"
        )]
        Errno::OPNOTSUPP => ErrorCode::Unsupported,
        _ => return None,
    })
}

/// The code for Windows' error number `raw`, where WASI names one, as `wasmtime-wasi` maps them;
/// a symlink where none may be followed is `loop`, as Unix's `ELOOP`.
#[cfg(windows)]
fn of_errno(raw: i32) -> Option<ErrorCode> {
    use windows_sys::Win32::Foundation;
    Some(match u32::try_from(raw).ok()? {
        Foundation::ERROR_FILE_NOT_FOUND => ErrorCode::NoEntry,
        Foundation::ERROR_PATH_NOT_FOUND => ErrorCode::NoEntry,
        Foundation::ERROR_ACCESS_DENIED => ErrorCode::Access,
        Foundation::ERROR_SHARING_VIOLATION => ErrorCode::Access,
        Foundation::ERROR_PRIVILEGE_NOT_HELD => ErrorCode::NotPermitted,
        Foundation::ERROR_INVALID_HANDLE => ErrorCode::BadDescriptor,
        Foundation::ERROR_INVALID_NAME => ErrorCode::NoEntry,
        Foundation::ERROR_NOT_ENOUGH_MEMORY => ErrorCode::InsufficientMemory,
        Foundation::ERROR_OUTOFMEMORY => ErrorCode::InsufficientMemory,
        Foundation::ERROR_DIR_NOT_EMPTY => ErrorCode::NotEmpty,
        Foundation::ERROR_NOT_READY => ErrorCode::Busy,
        Foundation::ERROR_BUSY => ErrorCode::Busy,
        Foundation::ERROR_NOT_SUPPORTED => ErrorCode::Unsupported,
        Foundation::ERROR_FILE_EXISTS => ErrorCode::Exist,
        Foundation::ERROR_BROKEN_PIPE => ErrorCode::Pipe,
        Foundation::ERROR_BUFFER_OVERFLOW => ErrorCode::NameTooLong,
        Foundation::ERROR_NOT_A_REPARSE_POINT => ErrorCode::Invalid,
        Foundation::ERROR_NEGATIVE_SEEK => ErrorCode::Invalid,
        Foundation::ERROR_DIRECTORY => ErrorCode::NotDirectory,
        Foundation::ERROR_ALREADY_EXISTS => ErrorCode::Exist,
        Foundation::ERROR_STOPPED_ON_SYMLINK => ErrorCode::Loop,
        Foundation::ERROR_DIRECTORY_NOT_SUPPORTED => ErrorCode::IsDirectory,
        _ => return None,
    })
}

/// The code for an error of `kind` with no number WASI names: [`beneath`](super::beneath)'s
/// refusal of a path that leaves its folder is `PermissionDenied`.
fn of_kind(kind: io::ErrorKind) -> ErrorCode {
    match kind {
        io::ErrorKind::NotFound => ErrorCode::NoEntry,
        io::ErrorKind::PermissionDenied => ErrorCode::NotPermitted,
        io::ErrorKind::AlreadyExists => ErrorCode::Exist,
        io::ErrorKind::InvalidInput => ErrorCode::Invalid,
        _ => ErrorCode::Io,
    }
}
