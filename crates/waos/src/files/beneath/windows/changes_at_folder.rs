//! The changes of a folder made on the handle of the folder that holds the name changed, which
//! [`open_parent`](crate::files::beneath::open_parent) opened beneath the mount: a folder made with
//! `NtCreateFile` relative to it, and what is removed, renamed or linked opened relative to it,
//! without following a link it is, and changed through its own handle (a disposition, a rename or a
//! link relative to the destination folder's handle), as `renameat` and `unlinkat` change a name on
//! Unix. cap-std's Windows half turns the folder's handle back into a path
//! (`GetFinalPathNameByHandleW`) and makes the standard library's path call on it, which Win32 then
//! parses again: a trailing dot or space dropped, a device name read as a device. WaOS's own, not
//! the copy's: written for the engine, on the NT calls Microsoft documents for drivers
//! (`ZwCreateFile`, `ZwSetInformationFile`).
//!
//! The invariants a later editor will break:
//! - No change reaches a path: every name here is one name, relative to a handle the walk checked.
//! - A name that is a link is changed itself, never what it leads to: every open here passes
//!   `FILE_FLAG_OPEN_REPARSE_POINT`.

use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::fs::MetadataExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::{Component, Path, PathBuf};
use std::{fs, io, mem, ptr};

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_DIRECTORY_FILE, FILE_LINK_INFORMATION, FILE_OPEN_REPARSE_POINT,
    FILE_RENAME_INFORMATION, FILE_RENAME_POSIX_SEMANTICS, FILE_RENAME_REPLACE_IF_EXISTS,
    FILE_SYNCHRONOUS_IO_NONALERT, FileLinkInformation, FileLinkInformationEx,
    FileRenameInformation, FileRenameInformationEx, NtCreateFile, NtSetInformationFile,
};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_DIRECTORY, ERROR_INVALID_NAME,
    ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, HANDLE, INVALID_HANDLE_VALUE, NTSTATUS,
    RtlNtStatusToDosError, UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_INFO_BY_HANDLE_CLASS,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, FileDispositionInfo, FileDispositionInfoEx,
    OPEN_EXISTING, SYNCHRONIZE, SetFileInformationByHandle,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::create_file_at_w::CreateFileAtW;
use super::read_link::read_reparse_point;

/// Makes the folder `name` in `folder`; `.` and `..` are there already, as Windows' own call
/// answers for them.
pub(crate) fn create_dir_at(folder: &fs::File, name: &Path) -> io::Result<()> {
    if matches!(
        name.components().next(),
        Some(Component::CurDir | Component::ParentDir)
    ) {
        return Err(io::Error::from_raw_os_error(ERROR_ALREADY_EXISTS as i32));
    }
    let name = one_name(name)?;
    let mut unicode = unicode_string(&name)?;
    // SAFETY: all-zero `OBJECT_ATTRIBUTES` and `IO_STATUS_BLOCK` are valid values, filled in below
    // or by the call.
    let mut attributes = unsafe { mem::zeroed::<OBJECT_ATTRIBUTES>() };
    attributes.Length = u32::try_from(mem::size_of::<OBJECT_ATTRIBUTES>()).expect("a size");
    attributes.RootDirectory = folder.as_raw_handle();
    attributes.ObjectName = &mut unicode;
    // SAFETY: as above.
    let mut status_block = unsafe { mem::zeroed::<IO_STATUS_BLOCK>() };
    let mut made: HANDLE = INVALID_HANDLE_VALUE;
    // SAFETY: the folder's open handle and a one-name path relative to it, which outlive the call;
    // the handle the call answers is owned and closed below.
    let status = unsafe {
        NtCreateFile(
            &mut made,
            FILE_LIST_DIRECTORY | SYNCHRONIZE,
            &attributes,
            &mut status_block,
            ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_CREATE,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            ptr::null(),
            0,
        )
    };
    succeeded(status)?;
    // SAFETY: the handle the call just opened, owned by nothing else.
    drop(unsafe { OwnedHandle::from_raw_handle(made) });
    Ok(())
}

/// Removes the empty folder `name` in `folder`, or the folder symlink it is.
pub(crate) fn remove_dir_at(folder: &fs::File, name: &Path) -> io::Result<()> {
    let entry = open_entry(folder, name, DELETE)?;
    if !is_folder(&entry)? {
        return Err(io::Error::from_raw_os_error(ERROR_DIRECTORY as i32));
    }
    delete(&entry)
}

/// Removes the file `name` in `folder`, or the file symlink it is; a folder, the folder symlinks
/// among them, is refused, as `DeleteFileW` refuses one.
pub(crate) fn remove_file_at(folder: &fs::File, name: &Path) -> io::Result<()> {
    let entry = open_entry(folder, name, DELETE)?;
    if is_folder(&entry)? {
        return Err(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED as i32));
    }
    delete(&entry)
}

/// Moves `name` in `folder` to `new_name` in `new_folder`, replacing what is there, as POSIX's
/// rename replaces it.
pub(crate) fn rename_at(
    folder: &fs::File,
    name: &Path,
    new_folder: &fs::File,
    new_name: &Path,
) -> io::Result<()> {
    let entry = open_entry(folder, name, DELETE | SYNCHRONIZE)?;
    let new_name = one_name(new_name)?;
    let flags = FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS;
    match set_name(
        &entry,
        FileRenameInformationEx,
        flags,
        new_folder,
        &new_name,
    ) {
        Err(error) if is_unsupported(&error) => {
            // A volume or a Windows without POSIX's rename: replacing alone.
            set_name(&entry, FileRenameInformation, 1, new_folder, &new_name)
        }
        renamed => renamed,
    }
}

/// Makes `new_name` in `new_folder` a hard link to the file `name` in `folder`; refused where a
/// name is there already.
pub(crate) fn hard_link_at(
    folder: &fs::File,
    name: &Path,
    new_folder: &fs::File,
    new_name: &Path,
) -> io::Result<()> {
    let entry = open_entry(folder, name, FILE_WRITE_ATTRIBUTES | SYNCHRONIZE)?;
    let new_name = one_name(new_name)?;
    match set_name(&entry, FileLinkInformationEx, 0, new_folder, &new_name) {
        Err(error) if is_unsupported(&error) => {
            set_name(&entry, FileLinkInformation, 0, new_folder, &new_name)
        }
        linked => linked,
    }
}

/// What the symlink or junction `name` in `folder` holds.
pub(crate) fn read_link_at(folder: &fs::File, name: &Path) -> io::Result<PathBuf> {
    read_reparse_point(&open_entry(folder, name, FILE_READ_ATTRIBUTES)?)
}

/// `name` in `folder`, opened with `access`, a folder too, and a link itself.
fn open_entry(folder: &fs::File, name: &Path, access: u32) -> io::Result<fs::File> {
    let name = one_name(name)?;
    // SAFETY: the folder's open handle and a one-name path relative to it, which outlive the call;
    // the handle it answers is owned by the file made of it.
    let handle = unsafe {
        CreateFileAtW(
            folder.as_raw_handle(),
            &name,
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    let handle: OwnedHandle = handle.try_into().map_err(|_| io::Error::last_os_error())?;
    Ok(fs::File::from(handle))
}

/// Whether `entry` is a folder, or a folder symlink.
fn is_folder(entry: &fs::File) -> io::Result<bool> {
    Ok(entry.metadata()?.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0)
}

/// Removes `entry`'s name now, as POSIX's unlink does, or once its last handle closes, on a volume
/// without POSIX's removal.
fn delete(entry: &fs::File) -> io::Result<()> {
    let posix = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    };
    match set_information(entry, FileDispositionInfoEx, &posix) {
        Err(error) if is_unsupported(&error) => {
            let delete = FILE_DISPOSITION_INFO { DeleteFile: true };
            set_information(entry, FileDispositionInfo, &delete)
        }
        deleted => deleted,
    }
}

/// Sets `class`'s information of `entry` to `information`.
fn set_information<T>(
    entry: &fs::File,
    class: FILE_INFO_BY_HANDLE_CLASS,
    information: &T,
) -> io::Result<()> {
    // SAFETY: an open handle and a structure of the class's type and size, which outlives the call.
    let set = unsafe {
        SetFileInformationByHandle(
            entry.as_raw_handle(),
            class,
            ptr::from_ref(information).cast(),
            u32::try_from(mem::size_of::<T>()).expect("a structure's size"),
        )
    };
    if set == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Gives `entry` the name `new_name` in `new_folder`, as a rename or a link by `class`, with
/// `flags` (`ReplaceIfExists` for a class without them): `FILE_RENAME_INFORMATION` and
/// `FILE_LINK_INFORMATION` are laid out alike, the name after the header, its length in bytes.
fn set_name(
    entry: &fs::File,
    class: i32,
    flags: u32,
    new_folder: &fs::File,
    new_name: &[u16],
) -> io::Result<()> {
    const _: () = assert!(
        mem::offset_of!(FILE_RENAME_INFORMATION, FileName)
            == mem::offset_of!(FILE_LINK_INFORMATION, FileName)
    );
    let header = mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
    let name_bytes = mem::size_of_val(new_name);
    let size = header + name_bytes;
    // `u64`s, so the structure, which holds a handle, is aligned for it.
    let mut buffer = vec![0u64; size.div_ceil(8)];
    let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // SAFETY: the buffer holds the header and the name, aligned for the structure; each field is
    // written in place, and the name after the header.
    unsafe {
        (*information).Anonymous.Flags = flags;
        (*information).RootDirectory = new_folder.as_raw_handle();
        (*information).FileNameLength = u32::try_from(name_bytes).expect("one name's length");
        ptr::copy_nonoverlapping(
            new_name.as_ptr(),
            buffer.as_mut_ptr().cast::<u8>().add(header).cast::<u16>(),
            new_name.len(),
        );
    }
    // SAFETY: all-zero `IO_STATUS_BLOCK` is a valid value, filled in by the call.
    let mut status_block = unsafe { mem::zeroed::<IO_STATUS_BLOCK>() };
    // SAFETY: an open handle, and the information of `size` bytes, which outlives the call.
    let status = unsafe {
        NtSetInformationFile(
            entry.as_raw_handle(),
            &mut status_block,
            buffer.as_ptr().cast(),
            u32::try_from(size).expect("one name's information"),
            class,
        )
    };
    succeeded(status)
}

/// `name`, which must be one name, as UTF-16 with no NUL.
fn one_name(name: &Path) -> io::Result<Vec<u16>> {
    if name.components().count() != 1 {
        return Err(io::Error::from_raw_os_error(ERROR_INVALID_NAME as i32));
    }
    Ok(name.as_os_str().encode_wide().collect())
}

/// `name` as the NT calls take it.
fn unicode_string(name: &[u16]) -> io::Result<UNICODE_STRING> {
    let length = u16::try_from(mem::size_of_val(name))
        .map_err(|_| io::Error::from_raw_os_error(ERROR_INVALID_NAME as i32))?;
    Ok(UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name.as_ptr().cast_mut(),
    })
}

/// `status` as an error where it is one, by the Win32 error it stands for.
fn succeeded(status: NTSTATUS) -> io::Result<()> {
    if status >= 0 {
        return Ok(());
    }
    // SAFETY: the call converts a status to an error number and touches nothing else.
    let error = unsafe { RtlNtStatusToDosError(status) };
    Err(io::Error::from_raw_os_error(error as i32))
}

/// Whether `error` says the volume, or this Windows, has no such information class.
fn is_unsupported(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error().map(|code| code as u32),
        Some(ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED)
    )
}
