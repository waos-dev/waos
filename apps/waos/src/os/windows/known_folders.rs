//! The user's local application data folder, `%LOCALAPPDATA%`, which stays on this machine, and
//! the Start menu's folder of the user's programs, as the shell names them
//! (`SHGetKnownFolderPath`).

// The shell's known folders are reached through a system call, and the path it answers is freed
// by COM's allocator: that is the `unsafe` here.
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_Programs, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows_sys::core::GUID;

/// `%LOCALAPPDATA%`; `None` where the shell names none.
pub(crate) fn local_application_data() -> Option<PathBuf> {
    known_folder(&FOLDERID_LocalAppData)
}

/// The Start menu's `Programs` folder of the user's, which Start lists and searches; `None` where
/// the shell names none.
pub(crate) fn start_menu_programs() -> Option<PathBuf> {
    known_folder(&FOLDERID_Programs)
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut path = ptr::null_mut();
    // SAFETY: the folder's id outlives the call, no token is the current user's, and the path the
    // shell answers is ours to free, whether or not it succeeded.
    let result =
        unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as u32, ptr::null_mut(), &mut path) };
    let folder = (result == 0 && !path.is_null()).then(|| {
        // SAFETY: the shell answers a NUL-terminated wide string.
        let length = (0..)
            .take_while(|&at| unsafe { *path.add(at) } != 0)
            .count();
        // SAFETY: `length` wide characters before the NUL, read before the free below.
        let wide = unsafe { std::slice::from_raw_parts(path, length) };
        PathBuf::from(OsString::from_wide(wide))
    });
    // SAFETY: the path the shell allocated with COM's allocator, freed once.
    unsafe { CoTaskMemFree(path.cast()) };
    folder
}
