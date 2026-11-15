//! The calls that name a path beneath a folder of the machine, kept beneath it: an open, a status,
//! a symlink read, and each change of a folder, refused when the path would leave the folder by
//! `..`, by an absolute path or by a symlink, as `wasmtime-wasi` refuses one leaving a preopened
//! folder. An open and a status keep their path there in one call where the system has one, each
//! system's route in [`unix`]: Apple's `O_RESOLVE_BENEATH` and `AT_RESOLVE_BENEATH`
//! (`unix/apple.rs`), FreeBSD's the same, and Linux's `openat2` with `RESOLVE_BENEATH`, each asked
//! of the system once; where it lacks the call, the path is walked one folder at a time, each
//! symlink read and followed ([`walk`]), as every path is on Windows, which has no such call
//! ([`windows`]). A change opens the folder holding the path's last name so, and makes the system's
//! call on that name ([`open_parent`]).
//!
//! The invariant: every route refuses the same paths with the same error, `PermissionDenied` with
//! no system error number ([`errors::escape_attempt`]), and reads a path's end alike, a `/` or `/.`
//! asking for a folder: a call that drops a path's end and lets the system read it is a way out, as
//! cap-std's rename of `link/` was (GHSA-m9f7-9w4p-gv38). `tests/routes.rs` holds every route to
//! the walk's verdicts, and a test runs the walk on any system by `walking`.
//!
//! Ported from `wasmtime-wasi` 50.0.1's `src/filesystem/primitives`, its copy of cap-std's path
//! code (`cap-primitives`), by the Wasmtime Project Developers, under the Apache License 2.0 with
//! the LLVM exception, which `files/LICENSE` holds: `mod.rs`, `errors.rs`, `file_type.rs`,
//! `maybe_owned_file.rs`, `metadata.rs`, `open_options.rs`, `open_parent.rs`,
//! `open_unchecked_error.rs`, `manually/`, `unix/`, `windows/` and `tests/`; `windows/` is
//! cap-primitives 4.0.3's `src/windows/fs/` as wasmtime copied it, reformatted, its imports from
//! `windows-sys` alone. Their names here, so that the next diff against wasmtime's is a rename
//! away:
//!
//! | `wasmtime-wasi`'s `primitives/` | here, `beneath/` |
//! | --- | --- |
//! | `mod.rs` and the files beside it | the same names |
//! | `manually/` | `walk/` |
//! | `unix/dir_utils.rs` | `unix/folders.rs` |
//! | `unix/oflags.rs` | `unix/open_flags.rs` |
//! | `windows/dir_utils.rs` | `windows/folders.rs` |
//! | `windows/oflags.rs` | `windows/open_flags.rs` |
//! | `tests/helpers/` | `tests/dir_calls/` |
//! | none | `unix/apple.rs`, `windows/changes_at_folder.rs`, `tests/routes.rs`, `tests/names_ending_in_a_slash.rs`, `tests/changes_at_folder.rs` |
//!
//! What differs:
//!
//! - Apple's systems have a route of their own (`unix/apple.rs`), beside FreeBSD's and Linux's, for
//!   an open and a status: `O_RESOLVE_BENEATH`, from macOS 15.4 and iOS 18.4, and
//!   `AT_RESOLVE_BENEATH`, from 26, each asked of the system once, as FreeBSD's is, and the walk
//!   where the system lacks one. An escape either refuses, `ENOTCAPABLE`, is
//!   [`errors::escape_attempt`], for a status as for an open, as the walk refuses it. The changes
//!   of a folder walk to the folder holding the last name, as wasmtime's do on Apple's systems.
//! - WASI's and VxWorks's branches, and `unix/cvt.rs`, which wasmtime compiles nowhere, are left
//!   out. Windows's half is the copy's, its `unsafe` the copy's own, allowed in `windows/` alone.
//! - On Windows a change of a folder, a folder made or removed, a file removed, a rename, a hard
//!   link, and a symlink read, is made on the handle of the folder holding the name
//!   (`windows/changes_at_folder.rs`), where the copy turns the handle back into a path and makes
//!   Win32's path call, which reads the path again, a trailing dot dropped and a device's name a
//!   device. A symlink is still made, and a folder still listed, through the path, as the copy does.
//! - On Windows, `tests/routes.rs` is left out, the walk being Windows's one route, and
//!   `tests/names_ending_in_a_slash.rs` makes folder symlinks, skipped without the privilege to make
//!   one, as the copy's tests are; seven of the copy's own tests make one unchecked, and fail
//!   without it.
//! - A time to set is made seconds and nanoseconds in `unix/times.rs` itself, as wasmtime's
//!   `clocks::Datetime` makes one.
//! - `OpenOptions::_cap_fs_ext_follow`, cap-fs-ext's way in, is left out.
//! - The tests write a fixed run of every byte's value where the copy's write random bytes, so that
//!   they need no `rand`; and the engine's own tests stand beside the copy's: `tests/routes.rs`,
//!   every route against the walk, and `tests/names_ending_in_a_slash.rs`, every change of a folder
//!   through `link/`.
//! - The copy is linted as `wasmtime-wasi`'s manifest lints it, and keeps its API whole, though the
//!   engine calls part of it and its tests the rest; what WaOS adds is linted as the rest of WaOS.

// The copy is linted as `wasmtime-wasi` lints it, every lint of clippy's allowed but those its
// manifest warns of, so that it stays wasmtime's code; what WaOS adds warns of every one.
#![allow(
    clippy::all,
    reason = "the copy is linted as wasmtime-wasi lints it, so that it stays wasmtime's code"
)]
#![warn(
    clippy::allow_attributes_without_reason,
    clippy::clone_on_copy,
    clippy::extra_unused_type_parameters,
    clippy::from_over_into,
    clippy::manual_strip,
    clippy::map_clone,
    clippy::multiple_bound_locations,
    clippy::redundant_field_names,
    clippy::uninlined_format_args,
    clippy::unnecessary_cast,
    clippy::unnecessary_fallible_conversions,
    clippy::unnecessary_mut_passed,
    clippy::unnecessary_to_owned,
    clippy::useless_conversion
)]
#![allow(
    clippy::unnecessary_fallible_conversions,
    reason = "typedefs are platform-specific"
)]
#![allow(trivial_numeric_casts, reason = "typedefs are platform-specific")]
#![allow(clippy::useless_conversion, reason = "typedefs are platform-specific")]
#![cfg_attr(
    not(test),
    allow(
        dead_code,
        unused_imports,
        reason = "the copy's API is kept whole, so that wasmtime-wasi's fixes port by a diff: the \
                  engine calls part of it, its tests the rest"
    )
)]

use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use std::{fs, io};

mod file_type;
mod maybe_owned_file;
mod metadata;
mod open_options;
mod open_parent;
mod open_unchecked_error;

mod errors;
mod walk;

#[cfg(test)]
mod tests;

#[cfg(not(windows))]
mod unix;
#[cfg(not(windows))]
use self::unix as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as sys;

use maybe_owned_file::MaybeOwnedFile;
use open_parent::open_parent;
use open_unchecked_error::*;
use sys::*;

pub(crate) use file_type::FileType;
pub(crate) use metadata::Metadata;
pub(crate) use open_options::*;
#[cfg(windows)]
pub(crate) use sys::file_path;
pub(crate) use sys::open_ambient_dir;
pub(crate) use sys::read_dir;
pub(crate) use sys::set_times;
pub(crate) use sys::set_times_nofollow;

/// Set the timestamps of the file or directory that `fd` itself refers to.
pub(crate) fn set_times_on_fd(
    fd: &fs::File,
    atime: Option<SystemTime>,
    mtime: Option<SystemTime>,
) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if sys::set_times_fast(fd, atime, mtime)? {
        return Ok(());
    }

    let mut times = fs::FileTimes::new();
    if let Some(atime) = atime {
        times = times.set_accessed(atime);
    }
    if let Some(mtime) = mtime {
        times = times.set_modified(mtime);
    }
    fd.set_times(times)
}

pub(crate) fn open(start: &fs::File, path: &Path, options: &OpenOptions) -> io::Result<fs::File> {
    #[cfg(any(
        windows,
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "android",
        target_os = "linux",
    ))]
    if !walks_every_path()
        && let Some(file) = sys::open_fast(start, path, options)?
    {
        return Ok(file);
    }
    walk::open(start, path, options)
}

#[cfg(test)]
thread_local! {
    /// Whether this thread's opens and statuses walk every path, whatever one call the system
    /// has: how a test runs the walk on every system, as Android and Windows run it.
    static WALKING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `test`, its opens and statuses on this thread walking every path, whatever one call the
/// system has.
#[cfg(test)]
pub(crate) fn walking<R>(test: impl FnOnce() -> R) -> R {
    WALKING.set(true);
    let tested = test();
    WALKING.set(false);
    tested
}

/// Whether every path is to be walked: in a test that asked for it ([`walking`]), never
/// elsewhere.
fn walks_every_path() -> bool {
    #[cfg(test)]
    return WALKING.get();
    #[cfg(not(test))]
    false
}

/// Should symlinks be followed in the last component of a path?
///
/// This doesn't affect path components other than the last. So for example in
/// "foo/bar/baz", if "foo" or "bar" are symlinks, they will always be
/// followed. This enum value only determines whether "baz" is followed.
///
/// Instead of passing bare `bool`s as parameters, pass a distinct enum so that
/// the intent is clear.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FollowSymlinks {
    /// Yes, do follow symlinks in the last component of a path.
    Yes,

    /// No, do not follow symlinks in the last component of a path.
    No,
}

/// Perform a `readlinkat`-like operation, ensuring that the resolution of the
/// path never escapes the directory tree rooted at `start`, and additionally
/// verifying that the link target is not absolute.
pub(crate) fn read_link(start: &fs::File, path: &Path) -> io::Result<PathBuf> {
    // On most platforms `read_link` is implemented by `open`ing up the parent
    // component of the path and then calling `read_link_unchecked` on the last
    // component.
    //
    // This technique doesn't work in all cases on Windows. In particular, a
    // directory symlink such as `C:\Documents and Settings` may not grant any
    // access other than what is needed to resolve the symlink, so
    // `open_parent`'s technique of returning a relative path of `.` from that
    // point doesn't work, because opening `.` within such a directory is
    // denied. Consequently, we use a different implementation on Windows.
    let contents = sys::read_link(start, path)?;

    // Don't allow reading symlinks to absolute paths. This isn't strictly
    // necessary to preserve the sandbox, since `open` will refuse to follow
    // absolute paths in any case. However, it is useful to enforce this
    // restriction to avoid leaking information about the host filesystem
    // outside the sandbox.
    if contents.has_root() {
        return Err(errors::escape_attempt());
    }

    Ok(contents)
}

/// Perform a `symlinkat`-like operation, ensuring that the resolution of the
/// path never escapes the directory tree rooted at `start`. An error is
/// returned if the target path is absolute.
#[cfg(not(windows))]
#[inline]
pub(crate) fn symlink(old_path: &Path, new_start: &fs::File, new_path: &Path) -> io::Result<()> {
    // Don't allow creating symlinks to absolute paths. This isn't strictly
    // necessary to preserve the sandbox, since `open` will refuse to follow
    // absolute symlinks in any case. However, it is useful to enforce this
    // restriction so that a WASI program can't trick some other non-WASI
    // program into following an absolute path.
    if old_path.has_root() {
        return Err(errors::escape_attempt());
    }

    let new_start = MaybeOwnedFile::borrowed(new_start);
    let (new_dir, new_basename) = open_parent(new_start, new_path)?;
    symlink_unchecked(old_path, &new_dir, new_basename.as_ref())
}

/// Perform a `symlink_file`-like operation, ensuring that the resolution of
/// the path never escapes the directory tree rooted at `start`.
#[cfg(windows)]
#[inline]
pub(crate) fn symlink_file(
    old_path: &Path,
    new_start: &fs::File,
    new_path: &Path,
) -> io::Result<()> {
    // As above, don't allow creating symlinks to absolute paths.
    if old_path.has_root() {
        return Err(errors::escape_attempt());
    }

    let new_start = MaybeOwnedFile::borrowed(new_start);
    let (new_dir, new_basename) = open_parent(new_start, new_path)?;
    symlink_file_unchecked(old_path, &new_dir, new_basename.as_ref())
}

/// Perform a `symlink_dir`-like operation, ensuring that the resolution of the
/// path never escapes the directory tree rooted at `start`.
#[cfg(windows)]
#[inline]
pub(crate) fn symlink_dir(
    old_path: &Path,
    new_start: &fs::File,
    new_path: &Path,
) -> io::Result<()> {
    // As above, don't allow creating symlinks to absolute paths.
    if old_path.has_root() {
        return Err(errors::escape_attempt());
    }

    let new_start = MaybeOwnedFile::borrowed(new_start);
    let (new_dir, new_basename) = open_parent(new_start, new_path)?;
    symlink_dir_unchecked(old_path, &new_dir, new_basename.as_ref())
}

/// Perform a `mkdirat`-like operation, ensuring that the resolution of the
/// path never escapes the directory tree rooted at `start`.
///
/// This is implemented by `open`ing up the parent component of the path and
/// then calling `create_dir_unchecked` on the last component.
pub(crate) fn create_dir(start: &fs::File, path: &Path) -> io::Result<()> {
    let start = MaybeOwnedFile::borrowed(start);

    // As a special case, `create_dir` ignores a trailing slash rather than
    // treating it as equivalent to a trailing slash-dot, so strip any trailing
    // slashes.
    let path = strip_dir_suffix(path);

    let (dir, basename) = open_parent(start, &path)?;

    create_dir_unchecked(&dir, basename.as_ref())
}

/// Perform a `linkat`-like operation, ensuring that the resolution of both
/// paths never escapes the directory tree rooted at their respective starts.
///
/// This is implemented by `open`ing up the parent component of each path and
/// then calling `hard_link_unchecked` on the last components.
pub(crate) fn hard_link(
    old_start: &fs::File,
    old_path: &Path,
    new_start: &fs::File,
    new_path: &Path,
) -> io::Result<()> {
    let old_start = MaybeOwnedFile::borrowed(old_start);
    let new_start = MaybeOwnedFile::borrowed(new_start);

    let (old_dir, old_basename) = open_parent(old_start, old_path)?;
    let (new_dir, new_basename) = open_parent(new_start, new_path)?;

    hard_link_unchecked(
        &old_dir,
        old_basename.as_ref(),
        &new_dir,
        new_basename.as_ref(),
    )
}

/// Perform a `renameat`-like operation, ensuring that the resolution of both
/// paths never escapes the directory tree rooted at their respective starts.
///
/// This is implemented by `open`ing up the parent component of each path and
/// then calling `rename_unchecked` on the last components.
pub(crate) fn rename(
    old_start: &fs::File,
    old_path: &Path,
    new_start: &fs::File,
    new_path: &Path,
) -> io::Result<()> {
    let old_start = MaybeOwnedFile::borrowed(old_start);
    let new_start = MaybeOwnedFile::borrowed(new_start);

    // As a special case, `rename` ignores a trailing slash rather than treating
    // it as equivalent to a trailing slash-dot, so strip any trailing slashes
    // for the purposes of `open_parent`.
    //
    // And remember whether either path ended with a slash so that we can still
    // fail if the source isn't a directory.
    let requires_dir = path_has_trailing_slash(old_path) || path_has_trailing_slash(new_path);
    let old_path = strip_dir_suffix(old_path);
    let new_path = strip_dir_suffix(new_path);

    let (old_dir, old_basename) = open_parent(old_start, &old_path)?;
    let (new_dir, new_basename) = open_parent(new_start, &new_path)?;

    // A trailing slash on either path requires the source to be a directory.
    //
    // On Linux, re-append a slash to the source and let the kernel check that
    // atomically with the rename. Linux never follows a symlink named with a
    // trailing slash in `renameat` and fails with `ENOTDIR` instead.
    #[cfg(target_os = "linux")]
    let mut concat;
    #[cfg(target_os = "linux")]
    let old_basename = if requires_dir {
        concat = old_basename.to_owned();
        concat.push("/");
        concat.as_os_str()
    } else {
        old_basename
    };

    // Elsewhere, such as on macOS and FreeBSD, `renameat` follows a symlink
    // named with a trailing slash. Check without following symlinks and rename
    // by the plain names instead, so such a symlink is never followed out of
    // `old_start`. The check isn't atomic with the rename, but whatever
    // replaces the source in between is still renamed by its plain name.
    #[cfg(not(target_os = "linux"))]
    if requires_dir
        && !stat_unchecked(&old_dir, Path::new(old_basename), FollowSymlinks::No)?.is_dir()
    {
        return Err(errors::is_not_directory());
    }

    rename_unchecked(
        &old_dir,
        old_basename.as_ref(),
        &new_dir,
        new_basename.as_ref(),
    )
}

/// Open a directory by performing an `openat`-like operation, ensuring that
/// the resolution of the path never escapes the directory tree rooted at
/// `start`.
#[inline]
fn open_dir(start: &fs::File, path: &Path) -> io::Result<fs::File> {
    open(start, path, &dir_options())
}

pub(crate) fn remove_file(start: &fs::File, path: &Path) -> io::Result<()> {
    #[cfg(target_os = "freebsd")]
    if sys::remove_file_fast(start, path)? {
        return Ok(());
    }

    let start = MaybeOwnedFile::borrowed(start);
    let (dir, basename) = open_parent(start, path)?;
    remove_file_unchecked(&dir, basename.as_ref())
}

pub(crate) fn remove_dir(start: &fs::File, path: &Path) -> io::Result<()> {
    #[cfg(target_os = "freebsd")]
    if sys::remove_dir_fast(start, path)? {
        return Ok(());
    }

    let start = MaybeOwnedFile::borrowed(start);
    let (dir, basename) = open_parent(start, path)?;
    remove_dir_unchecked(&dir, basename.as_ref())
}

pub(crate) fn stat(start: &fs::File, path: &Path, follow: FollowSymlinks) -> io::Result<Metadata> {
    // Optimization: if path has exactly one component and it's not ".." or
    // anything non-normal and we're not following symlinks we can go straight
    // to `stat_unchecked`, which can be faster than various paths below.
    if follow == FollowSymlinks::No && !path_requires_dir(path) {
        let mut components = path.components();
        if let Some(Component::Normal(component)) = components.next() {
            if components.next().is_none() {
                return stat_unchecked(start, component.as_ref(), FollowSymlinks::No);
            }
        }
    }

    #[cfg(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "android",
        target_os = "linux",
    ))]
    if !walks_every_path()
        && let Some(stat) = sys::stat_fast(start, path, follow)?
    {
        return Ok(stat);
    }

    walk::stat(start, path, follow)
}
