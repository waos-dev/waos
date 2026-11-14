//! Loading the window library and calling its entry, through the dynamic linker.
//!
//! The invariant: this is the one place in the loader where `unsafe` is written.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_int};

/// Loads the library at `library` and calls its entry `entry`; answers the entry's exit status,
/// or why the library or its entry could not be found.
pub(crate) fn run_entry(library: &str, entry: &str) -> Result<c_int, String> {
    let library = CString::new(library).map_err(|_| "a NUL in the path".to_owned())?;
    let entry = CString::new(entry).map_err(|_| "a NUL in the entry's name".to_owned())?;
    // SAFETY: both names are NUL-terminated; the library stays loaded for the process's life.
    let handle = unsafe { libc::dlopen(library.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        return Err(last_error());
    }
    // SAFETY: `handle` is a library just loaded, and the name is NUL-terminated.
    let symbol = unsafe { libc::dlsym(handle, entry.as_ptr()) };
    if symbol.is_null() {
        return Err(last_error());
    }
    // SAFETY: the window library exports its entry as `extern "C" fn() -> c_int`.
    let main: extern "C" fn() -> c_int = unsafe { std::mem::transmute(symbol) };
    Ok(main())
}

/// What the dynamic linker last said went wrong.
fn last_error() -> String {
    // SAFETY: `dlerror` answers a NUL-terminated message, or null when there is none.
    let message = unsafe { libc::dlerror() };
    if message.is_null() {
        return "the dynamic linker gave no reason".to_owned();
    }
    // SAFETY: a message `dlerror` just answered, which lives until the next call.
    unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned()
}
