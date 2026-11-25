//! The user's configuration folder for `waos`, where it keeps the signing key and the devices it
//! is paired with: `$XDG_CONFIG_HOME/waos`, or else `waos` in the system's own folder, where
//! command-line tools keep theirs; and its files made for the user alone, by mode on Unix and on
//! Windows by an access list naming the user alone.

use std::env;
#[cfg(unix)]
use std::fs::{DirBuilder, File, OpenOptions};
#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[cfg(windows)]
pub(crate) use crate::os::windows::{create_private_file, make_private_folder};

/// `waos`'s folder in the user's configuration.
pub fn folder() -> Result<PathBuf> {
    let configuration = match env::var_os("XDG_CONFIG_HOME") {
        Some(folder) if Path::new(&folder).is_absolute() => PathBuf::from(folder),
        _ => system_configuration()?,
    };
    Ok(configuration.join("waos"))
}

/// `~/.config` on macOS and Linux.
#[cfg(not(windows))]
fn system_configuration() -> Result<PathBuf> {
    Ok(env::home_dir()
        .context("this system names no home folder to keep waos's configuration in")?
        .join(".config"))
}

/// `%APPDATA%` on Windows, the user's roaming application data, which follows the user to every
/// machine of their domain, as a signing key that must not be lost should.
#[cfg(windows)]
fn system_configuration() -> Result<PathBuf> {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|folder| folder.is_absolute())
        .context("Windows names no APPDATA folder to keep waos's configuration in")
}

/// Makes `folder` and its parents, the folder for the user alone (`0700`); one already there is
/// left as it is.
#[cfg(unix)]
pub(crate) fn make_private_folder(folder: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(folder)
}

/// A new file at `path`, to write, which the user alone may read (`0600`); `AlreadyExists` when
/// there is one.
#[cfg(unix)]
pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}
