//! Apple's one call beneath a folder: `openat` with `O_RESOLVE_BENEATH`, which macOS has from 15.4
//! and iOS from 18.4, the release on the same kernel, and `fstatat` with `AT_RESOLVE_BENEATH`,
//! which both have from 26. The system is asked once whether it knows each flag, and where it does
//! not, the path is walked. The `libc` crate and rustix name the flags and their error for
//! FreeBSD alone; their values here are those of the SDK's `<sys/fcntl.h>` and `<sys/errno.h>`.
//!
//! The invariant: an escape the system refuses, with `ENOTCAPABLE`, is the error the walk refuses
//! one with, so that a program cannot tell which route served it.

// WaOS's own, beside the copy: linted as the rest of WaOS.
#![warn(clippy::all)]

use crate::files::beneath::unix::compute_oflags;
use crate::files::beneath::{FollowSymlinks, Metadata, OpenOptions, errors};
use rustix::fs::{AtFlags, Mode, OFlags, RawMode, openat, statat};
use rustix::io::Errno;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::OnceLock;
use tracing::debug;

/// The path may not leave the folder, by `..`, an absolute path or a symlink.
const O_RESOLVE_BENEATH: u32 = 0x1000;
const AT_RESOLVE_BENEATH: u32 = 0x2000;
/// What a call with either flag fails with when the path would leave the folder.
const ENOTCAPABLE: i32 = 107;

pub(crate) fn open_fast(
    start: &fs::File,
    path: &Path,
    options: &OpenOptions,
) -> io::Result<Option<fs::File>> {
    if !opens_beneath() {
        return Ok(None);
    }

    let oflags = compute_oflags(options)? | OFlags::from_bits_retain(O_RESOLVE_BENEATH);

    let mode = if oflags.contains(OFlags::CREATE) {
        Mode::from_bits((options.ext.mode & 0o7777) as RawMode).unwrap()
    } else {
        Mode::empty()
    };

    match openat(start, path, oflags, mode) {
        Ok(file) => Ok(Some(file.into())),
        Err(err) => Err(escape_as_the_walk_refuses(err)),
    }
}

pub(crate) fn stat_fast(
    start: &fs::File,
    path: &Path,
    follow: FollowSymlinks,
) -> io::Result<Option<Metadata>> {
    if !stats_beneath() {
        return Ok(None);
    }
    let flags = AtFlags::from_bits_retain(AT_RESOLVE_BENEATH)
        | if follow == FollowSymlinks::Yes {
            AtFlags::empty()
        } else {
            AtFlags::SYMLINK_NOFOLLOW
        };
    match statat(start, path, flags) {
        Ok(stat) => Ok(Some(Metadata::Stat(stat))),
        Err(err) => Err(escape_as_the_walk_refuses(err)),
    }
}

/// The system's error, with an escape made the walk's refusal of one.
fn escape_as_the_walk_refuses(err: Errno) -> io::Error {
    if err.raw_os_error() == ENOTCAPABLE {
        errors::escape_attempt()
    } else {
        err.into()
    }
}

/// Whether this system's `openat` knows `O_RESOLVE_BENEATH`.
pub(crate) fn opens_beneath() -> bool {
    static KNOWN: OnceLock<bool> = OnceLock::new();
    *KNOWN.get_or_init(|| {
        let flags = OFlags::RDONLY
            | OFlags::DIRECTORY
            | OFlags::CLOEXEC
            | OFlags::from_bits_retain(O_RESOLVE_BENEATH);
        let known = refuses_parent(|folder| openat(folder, "..", flags, Mode::empty()).map(drop));
        debug!(
            known,
            "whether openat keeps a path beneath its folder in one call"
        );
        known
    })
}

/// Whether this system's `fstatat` knows `AT_RESOLVE_BENEATH`.
pub(crate) fn stats_beneath() -> bool {
    static KNOWN: OnceLock<bool> = OnceLock::new();
    *KNOWN.get_or_init(|| {
        let flags = AtFlags::from_bits_retain(AT_RESOLVE_BENEATH);
        let known = refuses_parent(|folder| statat(folder, "..", flags).map(drop));
        debug!(
            known,
            "whether fstatat keeps a path beneath its folder in one call"
        );
        known
    })
}

/// Whether `reach`, reaching `..` from the temporary folder with a flag, is refused as an escape,
/// which only a system knowing the flag does: one that does not ignores it and reaches the parent,
/// or refuses the flag as invalid.
fn refuses_parent(reach: impl FnOnce(&fs::File) -> rustix::io::Result<()>) -> bool {
    let Ok(folder) = fs::File::open(std::env::temp_dir()) else {
        return false;
    };
    matches!(reach(&folder), Err(err) if err.raw_os_error() == ENOTCAPABLE)
}
