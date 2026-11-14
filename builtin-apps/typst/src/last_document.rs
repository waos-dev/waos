//! The document open when the program last ran, by its file's path, kept in the program's
//! `/data`: WaOS keeps a file picked or opened with the program among its mounts, so the path is
//! good at the next launch, and the document opens again.

use std::fs;
use std::path::{Path, PathBuf};

const KEPT_AT: &str = "/data/last-document";

/// Keeps `path` as the document to open at the next launch; none for an untitled document.
pub(crate) fn remember(path: Option<&Path>) {
    let kept = match path {
        Some(path) => fs::write(KEPT_AT, path.to_string_lossy().as_bytes()),
        None if Path::new(KEPT_AT).exists() => fs::remove_file(KEPT_AT),
        None => Ok(()),
    };
    if let Err(error) = kept {
        eprintln!("the last document is not remembered: {error}");
    }
}

/// The path kept at the last run.
pub(crate) fn remembered() -> Option<PathBuf> {
    let path = fs::read_to_string(KEPT_AT).ok()?;
    (!path.is_empty()).then(|| PathBuf::from(path))
}
