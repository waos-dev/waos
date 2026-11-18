//! A folder a crate carries a copy of, so the crate builds from what it publishes: the WIT
//! (`wit_copies`) and the website's docs (`docs_copy`). The source is the one place the files are
//! edited; `sync` replaces the copy whole, and `drift` names every file where the two differ.
//!
//! The invariant a later editor will break: a copy is its source less what `left_out` names, and
//! nothing else. Sync and drift read the same file list, so a path left out of one is left out of
//! the other.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A folder and where a crate keeps its copy, each from the workspace's root, written with `/`.
pub struct FolderCopy {
    pub source: &'static str,
    pub destination: &'static str,
    /// The source's paths not copied, each the start of a path from the source written with `/`:
    /// `docs/reference/wasdk-` leaves out every file under `docs/reference/wasdk-platform/`.
    pub left_out: &'static [&'static str],
}

impl FolderCopy {
    /// Replaces the copy with the source as it is now, leaving nothing of the copy before.
    pub fn sync(&self, root: &Path) -> io::Result<()> {
        let destination = root.join(self.destination);
        remove_folder_if_there(&destination)?;
        let source = root.join(self.source);
        for file in self.copied_files(&source)? {
            let to = destination.join(&file);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(source.join(&file), to)?;
        }
        Ok(())
    }

    /// Every file where the copy differs from its source, in the order of their paths.
    pub fn drift(&self, root: &Path) -> io::Result<Vec<Drift>> {
        let in_source = self.copied_files(&root.join(self.source))?;
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

    /// The files of `source` the copy holds: all of them but those `left_out` names.
    fn copied_files(&self, source: &Path) -> io::Result<BTreeSet<PathBuf>> {
        let mut files = files_under(source)?;
        files.retain(|file| !self.leaves_out(file));
        Ok(files)
    }

    fn leaves_out(&self, file: &Path) -> bool {
        let written: Vec<_> = file.iter().map(|part| part.to_string_lossy()).collect();
        let written = written.join("/");
        self.left_out.iter().any(|start| written.starts_with(start))
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

/// `path`, written with `/` as [`FolderCopy`]'s folders are, as this system writes a path, so a
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

fn remove_folder_if_there(folder: &Path) -> io::Result<()> {
    match fs::remove_dir_all(folder) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
