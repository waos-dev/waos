//! The WIT's copies. `wit/` is the one place the WIT is edited; each crate that generates
//! bindings or pages from it carries a copy of its own, so the crate builds from what it
//! publishes, and the repository checks out on Windows, where Git makes a symbolic link a plain file (the design's
//! decision 57, as wasmtime's crates carry theirs, copied by its `ci/vendor-wit.sh`).
//!
//! The invariant a later editor will break: [`COPIES`] names every copy. A crate whose
//! `generate!`, `bindgen!` or build script reads WIT taken from `wit/` is listed here, and nowhere edits its
//! copy: `cargo xtask sync-wit` replaces each one whole, and `tests/wit_copies.rs` names every
//! file of a copy that differs from `wit/`.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Every copy of the WIT, each a crate's that generates bindings from it.
pub const COPIES: [WitCopy; 5] = [
    // The SDK's, which `wit_bindgen::generate!` makes the program's API of.
    WitCopy {
        source: "wit",
        destination: "sdk/wasdk/wit",
    },
    // The engine's, which `wasmtime::component::bindgen!` makes the host's traits of.
    WitCopy {
        source: "wit",
        destination: "crates/waos/wit",
    },
    // The WebGPU host's `wasi:webgpu`, whose bindings the engine's reuse for that package, so
    // the two are generated from the same WIT.
    WitCopy {
        source: "wit/deps/wasi-webgpu-0.3.0-rc.2",
        destination: "crates/waos-webgpu-host/wit/deps/wasi-webgpu-0.3.0-rc.2",
    },
    // `cargo waos`'s, which its build makes the reference's pages of, for `cargo waos docs`.
    WitCopy {
        source: "wit/deps/wasdk-platform-1.0.0",
        destination: "crates/waos-cli/wit/wasdk-platform-1.0.0",
    },
    WitCopy {
        source: "wit/deps/wasdk-media-1.0.0",
        destination: "crates/waos-cli/wit/wasdk-media-1.0.0",
    },
];

/// A folder of the WIT and where a crate keeps its copy, each from the workspace's root.
pub struct WitCopy {
    pub source: &'static str,
    pub destination: &'static str,
}

impl WitCopy {
    /// Replaces the copy with the source as it is now, leaving nothing of the copy before.
    pub fn sync(&self, root: &Path) -> io::Result<()> {
        let destination = root.join(self.destination);
        remove_folder_if_there(&destination)?;
        copy_folder(&root.join(self.source), &destination)
    }

    /// Every file where the copy differs from its source, in the order of their paths.
    pub fn drift(&self, root: &Path) -> io::Result<Vec<Drift>> {
        let in_source = files_under(&root.join(self.source))?;
        let in_copy = files_under(&root.join(self.destination))?;
        let mut drift = Vec::new();
        for file in in_source.union(&in_copy) {
            let source = system_path(self.source).join(file);
            let copy = system_path(self.destination).join(file);
            match (in_source.contains(file), in_copy.contains(file)) {
                (true, true) if fs::read(root.join(&source))? != fs::read(root.join(&copy))? => {
                    drift.push(Drift::Differs { copy, source });
                }
                (true, false) => drift.push(Drift::Missing { copy, source }),
                (false, true) => drift.push(Drift::Extra { copy }),
                _ => {}
            }
        }
        Ok(drift)
    }
}

/// A file where a copy is not its source, by paths from the workspace's root.
#[derive(Debug, PartialEq, Eq)]
pub enum Drift {
    /// The copy's file holds something else than the source's.
    Differs { copy: PathBuf, source: PathBuf },
    /// The source has a file the copy lacks.
    Missing { copy: PathBuf, source: PathBuf },
    /// The copy has a file the source lacks.
    Extra { copy: PathBuf },
}

impl fmt::Display for Drift {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Drift::Differs { copy, source } => {
                write!(f, "{} differs from {}", copy.display(), source.display())
            }
            Drift::Missing { copy, source } => {
                write!(
                    f,
                    "{} is missing, a copy of {}",
                    copy.display(),
                    source.display()
                )
            }
            Drift::Extra { copy } => write!(f, "{} is in no source", copy.display()),
        }
    }
}

/// `path`, written with `/` as [`WitCopy`]'s folders are, as this system writes a path, so a
/// [`Drift`] names each file with one separator.
fn system_path(path: &str) -> PathBuf {
    path.split('/').collect()
}

/// Every file under `folder`, at any depth, by its path from `folder`; none where `folder` is
/// not there.
fn files_under(folder: &Path) -> io::Result<BTreeSet<PathBuf>> {
    let mut files = BTreeSet::new();
    if folder.exists() {
        add_files(folder, Path::new(""), &mut files)?;
    }
    Ok(files)
}

/// Adds every file under `folder` to `files`, by its path from the folder `relative` names.
fn add_files(folder: &Path, relative: &Path, files: &mut BTreeSet<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            add_files(&entry.path(), &path, files)?;
        } else {
            files.insert(path);
        }
    }
    Ok(())
}

/// Copies every file under `source` to the same path under `destination`, making its folders.
fn copy_folder(source: &Path, destination: &Path) -> io::Result<()> {
    for file in files_under(source)? {
        let to = destination.join(&file);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source.join(&file), to)?;
    }
    Ok(())
}

fn remove_folder_if_there(folder: &Path) -> io::Result<()> {
    match fs::remove_dir_all(folder) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
