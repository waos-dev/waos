//! Windows' calls `cargo waos` needs beyond the standard library: a folder and a file made for the
//! user alone, as a mode of `0700` and `0600` makes them on Unix, for the signing key; and a text
//! value of the registry, where the installer recorded where WaOS is.
//!
//! The invariants a later editor will break:
//! - A folder or a file made here carries its security from the moment it exists, given to the
//!   call that makes it, never set after, so there is no moment another user could open it.
//! - This module wraps the system's calls and is the one place in the crate where `unsafe` is
//!   written; every buffer the system allocates for an answer is freed here, once.

#![allow(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Makes the folder `path` for the user alone, and the files made in it after, as they inherit
/// it; its parents as the system makes them. A folder already there is left as it is.
pub(crate) fn make_private_folder(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let security = UserOnly::new("OICI")?;
    let wide = wide(path.as_os_str());
    // SAFETY: a NUL-terminated path and attributes whose descriptor lives through the call.
    if unsafe { CreateDirectoryW(wide.as_ptr(), &security.attributes()) } != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) {
        return Ok(());
    }
    Err(error)
}

/// A new file at `path`, to write, which the user alone may open; `AlreadyExists` when there is
/// one.
pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    let security = UserOnly::new("")?;
    let wide = wide(path.as_os_str());
    // SAFETY: a NUL-terminated path and attributes whose descriptor lives through the call; the
    // handle it answers is this function's to own.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            0,
            &security.attributes(),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a handle the call just opened, owned by nothing else.
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

/// Where the installer of the app named `name` installed it, as its entry among the installed
/// apps says (`InstallLocation` under `Software\Microsoft\Windows\CurrentVersion\Uninstall\<name>`),
/// the user's own first, then the machine's; none where neither lists it.
pub(crate) fn install_location(name: &str) -> io::Result<Option<PathBuf>> {
    let key = format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{name}");
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Some(location) = registry_text(hive, &key, "InstallLocation")? {
            // The installer writes it quoted.
            return Ok(Some(PathBuf::from(location.trim_matches('"'))));
        }
    }
    Ok(None)
}

/// The text `value` of the registry's `key` under `hive`; none where either is missing.
fn registry_text(hive: HKEY, key: &str, value: &str) -> io::Result<Option<String>> {
    let (key, value) = (wide(OsStr::new(key)), wide(OsStr::new(value)));
    let mut bytes = 0;
    // SAFETY: NUL-terminated names; asked with no buffer, the call says how many bytes the text
    // takes.
    let sized = unsafe {
        RegGetValueW(
            hive,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut bytes,
        )
    };
    if sized == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if sized != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(sized as i32));
    }
    let mut text = vec![0u16; (bytes as usize).div_ceil(2)];
    // SAFETY: the buffer holds `bytes` bytes and outlives the call, which writes at most that.
    let read = unsafe {
        RegGetValueW(
            hive,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            text.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if read != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(read as i32));
    }
    let length = text
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(text.len());
    Ok(Some(
        OsString::from_wide(&text[..length])
            .to_string_lossy()
            .into_owned(),
    ))
}

/// A security descriptor that makes the user this process runs as the owner of what is made
/// with it, and gives that user alone access, through a protected list that inherits nothing:
/// `O:<sid>D:P(A;<inheritance>;FA;;;<sid>)`.
struct UserOnly {
    descriptor: PSECURITY_DESCRIPTOR,
}

impl UserOnly {
    /// The descriptor whose one entry carries `inheritance`: `OICI` for a folder, whose files and
    /// folders inherit it, or none for a file.
    fn new(inheritance: &str) -> io::Result<UserOnly> {
        let sid = user_sid()?;
        let text = wide(OsStr::new(&format!(
            "O:{sid}D:P(A;{inheritance};FA;;;{sid})"
        )));
        let mut descriptor = ptr::null_mut();
        // SAFETY: a NUL-terminated text that outlives the call; the descriptor it allocates is
        // freed when this is dropped.
        let made = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if made == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(UserOnly { descriptor })
    }

    /// The attributes a call that makes something takes, pointing at this descriptor, which
    /// outlives them.
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: 0,
        }
    }
}

impl Drop for UserOnly {
    fn drop(&mut self) {
        // SAFETY: the descriptor the system allocated with `LocalAlloc`, freed once.
        unsafe { LocalFree(self.descriptor) };
    }
}

/// The SID of the user this process runs as, in its text form, `S-1-5-21-…`.
fn user_sid() -> io::Result<String> {
    let token = process_token()?;
    let mut needed = 0;
    // SAFETY: asked with no buffer, the call only says how many bytes the answer needs, failing
    // as the buffer is too small, which is expected.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    // `u64`s, so the answer, which holds pointers, is aligned for them.
    let mut answer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the buffer is `needed` bytes at least, and outlives the call.
    let read = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            answer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the system wrote a `TOKEN_USER` at the start of the buffer, aligned as `u64`s are,
    // whose SID points into the same buffer, which lives until the end of this function.
    let user = unsafe { &*answer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = ptr::null_mut();
    // SAFETY: the SID in the buffer; the text the call allocates is freed below, once read.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the text the call allocated, not used after.
    Ok(unsafe { take_system_text(text) })
}

/// The NUL-terminated text the system allocated at `text`, read, then freed.
///
/// # Safety
///
/// `text` is a NUL-terminated UTF-16 text the system allocated with `LocalAlloc`, which nothing
/// uses after.
unsafe fn take_system_text(text: *mut u16) -> String {
    // SAFETY: the text ends with a NUL, before which every unit is read.
    let length = unsafe { (0..).take_while(|&at| *text.add(at) != 0).count() };
    // SAFETY: `length` units the system wrote.
    let read = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
    // SAFETY: allocated with `LocalAlloc`, freed once.
    unsafe { LocalFree(text.cast()) };
    read
}

/// This process's access token, to read who it runs as.
fn process_token() -> io::Result<OwnedHandle> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the current process's pseudo handle, which needs no closing; the token handle the
    // call answers is this function's to own.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a handle the call just opened, owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

/// `text` as the system's calls take it: UTF-16, NUL-terminated.
fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain([0]).collect()
}

/// What a test reads of a file's or a folder's security: its owner and its access list, in the
/// descriptor's text form.
#[cfg(test)]
pub(crate) fn owner_and_access_text(path: &Path) -> io::Result<String> {
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};

    let wanted = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let wide = wide(path.as_os_str());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: a NUL-terminated path; the descriptor the call allocates is freed below.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            wanted,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let mut text = ptr::null_mut();
    // SAFETY: the descriptor read above; the text the call allocates is freed below.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            wanted,
            &mut text,
            ptr::null_mut(),
        )
    };
    // SAFETY: the descriptor the system allocated, freed once.
    unsafe { LocalFree(descriptor) };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the text the call allocated, not used after.
    Ok(unsafe { take_system_text(text) })
}

/// The user's SID, for a test to compare a descriptor with.
#[cfg(test)]
pub(crate) fn this_users_sid() -> String {
    user_sid().expect("this process's user")
}
