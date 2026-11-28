//! Why the package manager could not do what it was asked, in words the person installing can act
//! on.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use waos_package::{PackageError, ProgramId, PublisherKey, Version};

/// Why the package manager's own files could not be read or written.
#[derive(Debug)]
pub enum FolderError {
    /// Reading or writing the file at `path` failed.
    Io { path: PathBuf, error: io::Error },
    /// The file at `path` is not as the package manager writes it: changed by hand, or damaged.
    Unreadable { path: PathBuf, reason: String },
}

impl FolderError {
    /// For `map_err`: an IO error at `path`.
    pub(crate) fn io_at(path: &Path) -> impl FnOnce(io::Error) -> FolderError + '_ {
        move |error| FolderError::Io {
            path: path.to_path_buf(),
            error,
        }
    }

    /// For `map_err`: the file at `path` is not as the package manager writes it, for the reason
    /// `error` gives.
    pub(crate) fn unreadable_at<E: fmt::Display>(
        path: &Path,
    ) -> impl FnOnce(E) -> FolderError + '_ {
        move |error| FolderError::Unreadable {
            path: path.to_path_buf(),
            reason: error.to_string(),
        }
    }
}

impl fmt::Display for FolderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FolderError::Io { path, error } => write!(f, "{}: {error}", path.display()),
            FolderError::Unreadable { path, reason } => {
                write!(f, "{} is not as WaOS wrote it: {reason}", path.display())
            }
        }
    }
}

/// The causes are in each message already, so a chain of errors does not say them twice.
impl Error for FolderError {}

/// Why an install did not install.
#[derive(Debug)]
pub enum InstallError {
    /// The package could not be opened, verified or unpacked.
    Package(PackageError),
    /// The app is installed from a package signed with another key: this package is not an
    /// update of it.
    DifferentPublisher {
        id: ProgramId,
        installed: PublisherKey,
        package: PublisherKey,
    },
    /// The package's version is not newer than the one installed.
    NotNewer {
        id: ProgramId,
        installed: Version,
        package: Version,
    },
    /// The package is of another app than the one its install was asked for.
    OtherApp {
        expected: ProgramId,
        package: ProgramId,
    },
    /// The package's bytes did not all arrive, or none did: what their stream said.
    Receiving(io::Error),
    /// Its component did not compile, in the compiler's words.
    Compile(anyhow::Error),
    /// Its session was cancelled: by its caller, by a newer session of the same caller for the
    /// app, or by the app's removal.
    Cancelled,
    /// The package manager's own files could not be read or written.
    Folder(FolderError),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::Package(error) => write!(f, "{error}"),
            InstallError::DifferentPublisher {
                id,
                installed,
                package,
            } => write!(
                f,
                "{id} is installed from the publisher whose key is {}; this package is signed with another key, {}, so it is not an update of it",
                installed.fingerprint(),
                package.fingerprint()
            ),
            InstallError::NotNewer {
                id,
                installed,
                package,
            } => write!(
                f,
                "{id} {installed} is installed, and this package is {package}, which is not newer"
            ),
            InstallError::OtherApp { expected, package } => {
                write!(f, "the package is of {package}, not of {expected}")
            }
            InstallError::Receiving(error) => {
                write!(f, "the package did not arrive: {error}")
            }
            InstallError::Compile(error) => write!(f, "it does not compile: {error:#}"),
            InstallError::Cancelled => write!(f, "the install was cancelled"),
            InstallError::Folder(error) => write!(f, "{error}"),
        }
    }
}

/// The causes are in each message already, so a chain of errors does not say them twice.
impl Error for InstallError {}

impl From<PackageError> for InstallError {
    fn from(error: PackageError) -> InstallError {
        InstallError::Package(error)
    }
}

impl From<FolderError> for InstallError {
    fn from(error: FolderError) -> InstallError {
        InstallError::Folder(error)
    }
}

/// Why a removal did not remove.
#[derive(Debug)]
pub enum RemoveError {
    /// No app of this id is installed.
    NotInstalled(ProgramId),
    /// The package manager's own files could not be read or written.
    Folder(FolderError),
}

impl fmt::Display for RemoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoveError::NotInstalled(id) => write!(f, "no app {id} is installed"),
            RemoveError::Folder(error) => write!(f, "{error}"),
        }
    }
}

/// The causes are in each message already, so a chain of errors does not say them twice.
impl Error for RemoveError {}

impl From<FolderError> for RemoveError {
    fn from(error: FolderError) -> RemoveError {
        RemoveError::Folder(error)
    }
}
