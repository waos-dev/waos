//! The user's environment as Windows builds it for a new logon (`CreateEnvironmentBlock`): the
//! system's variables and the user's from the registry, as a new terminal or a process started from
//! Explorer gets them, with the changes made since WaOS started. Windows has no login shell to
//! read it from, and VS Code reads none there either.
//!
//! The invariant: the block the system allocates is freed once, after it is read whole.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::ptr;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_QUERY};
use windows_sys::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::login_environment::Variables;

/// The environment Windows builds for the user this process runs as, without this process's own.
/// The variables naming a drive's current folder (`=C:`), which only a console's shell keeps, are
/// left out.
pub(crate) fn user_environment() -> io::Result<Variables> {
    let token = process_token()?;
    let mut block = ptr::null_mut();
    // SAFETY: the process's token, opened with the access the call asks for; the block it allocates
    // is destroyed below, once read.
    if unsafe { CreateEnvironmentBlock(&mut block, token.as_raw_handle(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a block the call just built: UTF-16 strings, each NUL-terminated, a NUL after the last.
    let variables = unsafe { variables_in(block.cast()) };
    // SAFETY: the block the call allocated, destroyed once.
    unsafe { DestroyEnvironmentBlock(block) };
    Ok(variables)
}

/// The `name=value` strings of an environment block.
///
/// # Safety
///
/// `block` points at UTF-16 strings, each NUL-terminated, followed by an empty one.
unsafe fn variables_in(block: *const u16) -> Variables {
    let mut variables = Variables::new();
    let mut at = block;
    loop {
        // SAFETY: each string ends with a NUL, before which every unit is read.
        let length = unsafe { (0..).take_while(|&offset| *at.add(offset) != 0).count() };
        if length == 0 {
            return variables;
        }
        // SAFETY: `length` units of the block.
        let entry = unsafe { std::slice::from_raw_parts(at, length) };
        if let Some(variable) = variable_of(entry) {
            variables.push(variable);
        }
        // SAFETY: the next string starts after this one's NUL, inside the block.
        at = unsafe { at.add(length + 1) };
    }
}

/// The name and the value of `name=value`; none for a drive's current folder, whose name starts
/// with `=`.
fn variable_of(entry: &[u16]) -> Option<(OsString, OsString)> {
    let equals = entry
        .iter()
        .skip(1)
        .position(|&unit| unit == u16::from(b'='))?
        + 1;
    if entry[0] == u16::from(b'=') {
        return None;
    }
    Some((
        OsString::from_wide(&entry[..equals]),
        OsString::from_wide(&entry[equals + 1..]),
    ))
}

/// This process's access token, to build its user's environment from.
fn process_token() -> io::Result<OwnedHandle> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the current process's pseudo handle, which needs no closing; the token the call
    // answers is this function's to own.
    let opened = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut token,
        )
    };
    if opened == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a handle the call just opened, owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variable_is_split_at_its_first_equals_and_a_drive_s_folder_is_left_out() {
        let wide = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(
            variable_of(&wide("PATH=C:\\a=b")),
            Some(("PATH".into(), "C:\\a=b".into()))
        );
        assert_eq!(variable_of(&wide("=C:=C:\\Users")), None);
    }

    #[test]
    fn the_user_s_environment_names_their_profile() {
        let variables = user_environment().unwrap();
        assert!(
            variables
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("USERPROFILE")),
            "{variables:?}"
        );
    }
}
