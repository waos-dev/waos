//! The names a program's path may hold beneath its mounts, where the system reads some otherwise
//! than WASI. On Windows a `\` is a separator and a `:` a drive or a file's stream, a name ending
//! in a dot or a space loses them, and `CON`, `NUL`, `COM1` and the rest name a device, with any
//! extension: each could name another file than the one the program named, so a path with such a
//! name is refused, as Windows itself refuses an invalid name (`ERROR_INVALID_NAME`, WASI's
//! `no-entry`). Elsewhere every name but `/` and NUL is a name, and nothing is refused here.
//!
//! The invariant: every name of the path is checked, not only its last, before any of it reaches
//! the system.

use super::errors::ErrorCode;

/// Refuses `path`, a program's, where one of its names would name another file on this system.
#[cfg(windows)]
pub(crate) fn check(path: &str) -> Result<(), ErrorCode> {
    if path.split('/').all(is_a_name_windows_keeps) {
        Ok(())
    } else {
        Err(ErrorCode::from(std::io::Error::from_raw_os_error(
            windows_sys::Win32::Foundation::ERROR_INVALID_NAME as i32,
        )))
    }
}

#[cfg(not(windows))]
pub(crate) fn check(_path: &str) -> Result<(), ErrorCode> {
    Ok(())
}

/// The devices Windows names in every folder, by a file's name before its first dot.
#[cfg(windows)]
const DEVICES: [&str; 32] = [
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// Whether Windows keeps `name`, one of a path's, as the name it is: `.`, `..` and the empty name
/// between two slashes are kept as the walk reads them.
#[cfg(windows)]
fn is_a_name_windows_keeps(name: &str) -> bool {
    if matches!(name, "" | "." | "..") {
        return true;
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    !name.contains(['\\', ':'])
        && !name.ends_with(['.', ' '])
        && !DEVICES
            .iter()
            .any(|device| device.eq_ignore_ascii_case(stem))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_name_windows_reads_otherwise_is_refused_wherever_it_is_in_the_path() {
        for refused in [
            r"a\b",
            "dir/a\\..\\..\\b",
            "c:file",
            "file.txt:stream",
            "trailing.",
            "trailing ",
            "dir./file",
            "NUL",
            "nul.txt",
            "dir/com1/file",
            "LPT9.log",
            "CONIN$",
        ] {
            assert_eq!(check(refused), Err(ErrorCode::NoEntry), "{refused}");
        }
    }

    #[test]
    fn ordinary_names_and_dots_are_kept() {
        for kept in [
            "file.txt",
            "dir/./file",
            "../file",
            "a//b/",
            ".hidden",
            "console.log",
            "nullable",
            "COM10",
            "名前",
        ] {
            assert_eq!(check(kept), Ok(()), "{kept}");
        }
    }
}
