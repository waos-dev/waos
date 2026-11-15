//! The user database's entry for the user running WaOS, on Unix's systems: the shell the user
//! logs in to, the one SSH and `login` start, which may differ from the `SHELL` WaOS was started
//! with. Both a machine's `login-shell` and the login environment its commands run in take it.
//!
//! The invariant: the entry is read with the reentrant call into a buffer of our own, so another
//! thread's lookup never overwrites it while it is read.

// `getpwuid_r` is the C library's, reached through `unsafe`; this module is the one place in the
// crate outside `windows` that writes it.
#![allow(unsafe_code)]

use std::ffi::CStr;
use std::mem::MaybeUninit;
use std::ptr;

/// What an empty shell in the user database stands for, as SSH and `login` read it.
const DEFAULT_SHELL: &str = "/bin/sh";

/// How large a buffer the first lookup is given; it doubles while the entry does not fit.
const FIRST_BUFFER: usize = 1024;

/// The user's login shell, as the user database names it; none where it has no entry for them.
pub(crate) fn login_shell() -> Option<String> {
    let user = rustix::process::getuid().as_raw();
    let mut buffer = vec![0 as libc::c_char; FIRST_BUFFER];
    loop {
        let mut entry = MaybeUninit::<libc::passwd>::uninit();
        let mut found = ptr::null_mut();
        // SAFETY: every pointer is to memory of ours, alive for the call, `buffer` of the length
        // given; the C library writes the entry into `entry`, its strings into `buffer`.
        let error = unsafe {
            libc::getpwuid_r(
                user,
                entry.as_mut_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut found,
            )
        };
        if error == libc::ERANGE {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if error != 0 || found.is_null() {
            return None;
        }
        // SAFETY: the lookup found the entry, so `entry` is written, and its shell is null or a
        // string in `buffer`, which lives until the end of this function.
        let shell = unsafe { entry.assume_init().pw_shell };
        if shell.is_null() {
            return Some(DEFAULT_SHELL.to_owned());
        }
        // SAFETY: as above, a string the C library ended with a nul, in `buffer`.
        let shell = unsafe { CStr::from_ptr(shell) }.to_string_lossy();
        return Some(if shell.is_empty() {
            DEFAULT_SHELL.to_owned()
        } else {
            shell.into_owned()
        });
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    /// The user database asked through the system's own command: `dscl` on macOS, `getent`
    /// elsewhere.
    fn shell_the_system_names() -> String {
        let script = if cfg!(target_vendor = "apple") {
            "dscl . -read \"/Users/$(id -un)\" UserShell | sed 's/^UserShell: //'"
        } else {
            "getent passwd \"$(id -u)\" | cut -d: -f7"
        };
        let output = Command::new("/bin/sh")
            .args(["-c", script])
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn the_login_shell_is_the_one_the_user_database_names() {
        let named = shell_the_system_names();
        let expected = if named.is_empty() {
            DEFAULT_SHELL
        } else {
            &named
        };
        assert_eq!(login_shell().as_deref(), Some(expected));
    }
}
