//! Where downloaded docs are kept: a folder for each version, `<cache>/waos/docs/<version>`, in the
//! user's cache folder (`~/Library/Caches` on macOS, `%LOCALAPPDATA%` on Windows,
//! `$XDG_CACHE_HOME` or `~/.cache` elsewhere), each the unpacked archive of that version's release.
//!
//! The invariant: a version's folder is there only whole. An archive is unpacked beside it under
//! another name and renamed into place, so a download cut short leaves no folder that looks done.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;

/// The file every unpacked archive has, which says the folder is one.
const MANIFEST: &str = "manifest.json";

/// The folder the versions' folders are in.
pub fn root() -> Result<PathBuf> {
    Ok(user_cache()?.join("waos").join("docs"))
}

/// The folder of `version`'s docs under `root`, if they are there.
pub fn bundle(root: &Path, version: &str) -> Option<PathBuf> {
    let folder = root.join(version);
    folder.join(MANIFEST).is_file().then_some(folder)
}

/// Unpacks `archive`, a gzipped tar, as `version`'s folder under `root`, and answers it.
pub fn install(root: &Path, version: &str, archive: &[u8]) -> Result<PathBuf> {
    let folder = root.join(version);
    let unpacking = root.join(format!(".{version}.{}", process::id()));
    remove_folder_if_there(&unpacking)?;
    fs::create_dir_all(&unpacking).with_context(|| format!("making {}", unpacking.display()))?;
    let unpacked = tar::Archive::new(GzDecoder::new(archive))
        .unpack(&unpacking)
        .context("unpacking the docs' archive")
        .and_then(|()| {
            anyhow::ensure!(
                unpacking.join(MANIFEST).is_file(),
                "the docs' archive holds no {MANIFEST}"
            );
            Ok(())
        });
    if let Err(error) = unpacked {
        remove_folder_if_there(&unpacking)?;
        return Err(error);
    }
    remove_folder_if_there(&folder)?;
    fs::rename(&unpacking, &folder).with_context(|| format!("putting {}", folder.display()))?;
    Ok(folder)
}

/// The user's cache folder, as the system names it.
fn user_cache() -> Result<PathBuf> {
    if let Some(folder) = env::var_os("XDG_CACHE_HOME").filter(|folder| !folder.is_empty()) {
        return Ok(PathBuf::from(folder));
    }
    if cfg!(windows) {
        let local = env::var_os("LOCALAPPDATA").context("no LOCALAPPDATA to keep the docs in")?;
        return Ok(PathBuf::from(local));
    }
    let home = env::home_dir().context("no home folder to keep the docs in")?;
    Ok(if cfg!(target_os = "macos") {
        home.join("Library").join("Caches")
    } else {
        home.join(".cache")
    })
}

fn remove_folder_if_there(folder: &Path) -> io::Result<()> {
    match fs::remove_dir_all(folder) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
