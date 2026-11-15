//! What a program's files need of Windows beyond [`beneath`](super::beneath) and the standard
//! library: a handle's link count and identity, which Windows tells of a handle rather than in its
//! metadata; a handle opened only to read what a path names, its status, without following a
//! symlink it ends in where asked; a write at the end of a file through a handle reopened to
//! append; a folder of the machine opened as a folder; and a symlink made, or removed, as the
//! file or folder link Windows tells apart.
//!
//! Ported from `wasmtime-wasi` 50.0.1's `src/filesystem/windows.rs` (`by_handle_info`,
//! `open_metadata_handle`, `append_cursor_unspecified`, `symlink`, `remove_file_or_symlink`), by the
//! Wasmtime Project Developers, under the Apache License 2.0 with the LLVM exception, which
//! `files/LICENSE` holds; the rest of that file serves what the engine does not.
//!
//! The invariant: wrapping these system calls is what this module is for, and its `unsafe` stays
//! here, each block over one call.

#![allow(unsafe_code)]

use std::fs;
use std::io::{self, Write as _};
use std::mem;
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
use std::path::Path;

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_WRITE_DATA, GetFileInformationByHandle, ReOpenFile,
};

use super::beneath::{self, FollowSymlinks, OpenOptions, OpenOptionsExt as _};

/// What Windows tells of a file through a handle and not in its metadata.
pub(crate) struct HandleInformation {
    pub(crate) link_count: u64,
    /// The serial number of the volume the file is on, and the file's index on it, which no other
    /// file on that volume shares while it exists.
    pub(crate) volume: u64,
    pub(crate) index: u64,
}

/// What Windows tells of `file` through its handle.
pub(crate) fn handle_information(file: &fs::File) -> io::Result<HandleInformation> {
    // SAFETY: an all-zero `BY_HANDLE_FILE_INFORMATION` is a valid value, which the call fills in.
    let mut information = unsafe { mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    // SAFETY: an open handle, and a structure of the call's own type that outlives it.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(HandleInformation {
        link_count: u64::from(information.nNumberOfLinks),
        volume: u64::from(information.dwVolumeSerialNumber),
        index: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

/// A handle on what `path` names beneath `folder`, opened with no access but to read its status:
/// a folder too, and a symlink it ends in itself, unless `follow`.
pub(crate) fn status_handle_beneath(
    folder: &fs::File,
    path: &Path,
    follow: FollowSymlinks,
) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.access_mode(0);
    options.custom_flags(status_flags(follow == FollowSymlinks::Yes));
    beneath::open(folder, path, &options)
}

/// A handle on what `path` names on the machine, as [`status_handle_beneath`] opens one, the path
/// resolved as the system resolves any path.
pub(crate) fn status_handle_on_machine(path: &Path, follow: bool) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(status_flags(follow))
        .open(path)
}

/// The flags an open for a status takes: a folder opened as any file, and a symlink itself where
/// it is not followed.
fn status_flags(follow: bool) -> u32 {
    if follow {
        FILE_FLAG_BACKUP_SEMANTICS
    } else {
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT
    }
}

/// The folder at `path` on the machine, opened for paths beneath it: as a folder, which Windows
/// opens only with `FILE_FLAG_BACKUP_SEMANTICS`, and not to be deleted or renamed while it is held.
pub(crate) fn open_folder(path: &Path) -> io::Result<fs::File> {
    beneath::open_ambient_dir(path)
}

/// Writes `bytes` at the end of `file`, wherever its end is when the write is made: through a
/// handle reopened on the same file to append, as Windows has no flag to switch on an open one.
pub(crate) fn append(file: &fs::File, bytes: &[u8]) -> io::Result<usize> {
    // SAFETY: an open handle; the handle the call answers is this function's to own, and is closed
    // when the file made of it is dropped.
    let appending = unsafe {
        ReOpenFile(
            file.as_raw_handle(),
            FILE_GENERIC_WRITE & !FILE_WRITE_DATA,
            // The share mode the standard library opens every file with.
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            0,
        )
    };
    if appending == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a handle the call just opened, owned by nothing else.
    let mut appending = unsafe { fs::File::from_raw_handle(appending) };
    appending.write(bytes)
}

/// Makes a symlink at `link` beneath `folder` holding `original`, a folder's link where `original`
/// names a folder and a file's otherwise, as Windows tells the two apart.
pub(crate) fn symlink(original: &Path, folder: &fs::File, link: &Path) -> io::Result<()> {
    if beneath::stat(folder, original, FollowSymlinks::Yes)?.is_dir() {
        beneath::symlink_dir(original, folder, link)
    } else {
        beneath::symlink_file(original, folder, link)
    }
}

/// Removes the file or the symlink at `path` beneath `folder`: a folder's symlink as a folder,
/// which Windows removes only so.
pub(crate) fn remove_file_or_symlink(folder: &fs::File, path: &Path) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.access_mode(DELETE);
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    let file = beneath::open(folder, path, &options)?;
    let metadata = file.metadata()?;
    let is_folder_link = metadata.file_type().is_symlink()
        && metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0;
    if is_folder_link {
        beneath::remove_dir(folder, path)?;
    } else {
        beneath::remove_file(folder, path)?;
    }
    // Held until the removal is made, so that nothing replaces the file under it: Windows removes a
    // file once its last handle closes.
    drop(file);
    Ok(())
}
