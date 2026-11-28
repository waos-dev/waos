//! Why the store could not read or write its files, in words the person using WaOS can act on.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Why the store's own files could not be read or written.
#[derive(Debug)]
pub enum StoreError {
    /// Reading or writing the file at `path` failed.
    Io { path: PathBuf, error: io::Error },
}

impl StoreError {
    /// For `map_err`: an IO error at `path`.
    pub(crate) fn io_at(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
        move |error| StoreError::Io {
            path: path.to_path_buf(),
            error,
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

/// The causes are in each message already, so a chain of errors does not say them twice.
impl Error for StoreError {}
