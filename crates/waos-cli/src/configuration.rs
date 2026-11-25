//! The user's configuration folder for `waos`, where it keeps the signing key and the devices it
//! is paired with: `$XDG_CONFIG_HOME/waos`, or else `waos` in the system's own folder, where
//! command-line tools keep theirs.

use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

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
