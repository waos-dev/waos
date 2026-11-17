//! Where the system says an open handle is now: `F_GETPATH` on Apple's systems, `/proc/self/fd` on
//! Linux and Android, `GetFinalPathNameByHandleW` on Windows. A rename moves the answer with the
//! file; a removed file is answered by its last path, which Android follows with ` (deleted)`
//! (measured with `fcntl` on macOS and `readlink` on Android).

use std::fs;
use std::io;
use std::path::PathBuf;

/// The path the system gives of what `handle` is open on.
#[cfg(target_vendor = "apple")]
pub(crate) fn path_of(handle: &fs::File) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt as _;

    let path = rustix::fs::getpath(handle)?;
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes())))
}

/// The path the system gives of what `handle` is open on: where its descriptor's link in
/// `/proc/self/fd` leads.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn path_of(handle: &fs::File) -> io::Result<PathBuf> {
    use std::os::fd::AsRawFd as _;

    fs::read_link(format!("/proc/self/fd/{}", handle.as_raw_fd()))
}

/// The path the system gives of what `handle` is open on.
#[cfg(windows)]
pub(crate) fn path_of(handle: &fs::File) -> io::Result<PathBuf> {
    super::beneath::file_path(handle).ok_or_else(|| io::ErrorKind::NotFound.into())
}
