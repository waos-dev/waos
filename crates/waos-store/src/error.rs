//! Why the store could not do what it was asked, in words the person using the library can act
//! on.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use reqwest::StatusCode;
use url::Url;
use waos_package::{IndexError, PackageError, ProgramId, PublisherKey, Version};

use crate::Source;

/// Why the store's own files could not be read or written.
#[derive(Debug)]
pub enum StoreError {
    /// Reading or writing the file at `path` failed.
    Io { path: PathBuf, error: io::Error },
    /// The file at `path` is not as the store writes it: changed by hand, or damaged.
    Unreadable { path: PathBuf, reason: String },
    /// No program of this id is installed.
    NotInstalled(ProgramId),
    /// The source is not among the sources added.
    NotAdded(Source),
    /// The HTTP client the store reads sources with could not start, in its words.
    Http(reqwest::Error),
}

impl StoreError {
    /// For `map_err`: an IO error at `path`.
    pub(crate) fn io_at(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
        move |error| StoreError::Io {
            path: path.to_path_buf(),
            error,
        }
    }

    /// For `map_err`: the file at `path` is not as the store writes it, for the reason `error`
    /// gives.
    pub(crate) fn unreadable_at<E: fmt::Display>(path: &Path) -> impl FnOnce(E) -> StoreError + '_ {
        move |error| StoreError::Unreadable {
            path: path.to_path_buf(),
            reason: error.to_string(),
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io { path, error } => write!(f, "{}: {error}", path.display()),
            StoreError::Unreadable { path, reason } => {
                write!(f, "{} is not as WaOS wrote it: {reason}", path.display())
            }
            StoreError::NotInstalled(id) => write!(f, "no program {id} is installed"),
            StoreError::NotAdded(source) => write!(f, "{source} is not one of the sources"),
            StoreError::Http(error) => {
                write!(f, "the HTTP client could not start: {}", with_causes(error))
            }
        }
    }
}

/// The causes are in each message already, so a chain of errors does not say them twice.
impl Error for StoreError {}

/// Why a package was not installed.
#[derive(Debug)]
pub enum InstallError {
    /// The package could not be opened, verified or unpacked.
    Package(PackageError),
    /// The program is installed from a package signed with another key: this package is not an
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
    /// A source's package is not the program at the version the source lists it as: each as
    /// its id and version, `dev.example.triangle 1.2.0`.
    NotAsListed { listed: String, package: String },
    /// The package could not be downloaded from its source.
    Download(SourceError),
    /// The store's own files could not be read or written.
    Store(StoreError),
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
            InstallError::NotAsListed { listed, package } => {
                write!(f, "the source lists {listed}, but its package is {package}")
            }
            InstallError::Download(error) => write!(f, "{error}"),
            InstallError::Store(error) => write!(f, "{error}"),
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

impl From<StoreError> for InstallError {
    fn from(error: StoreError) -> InstallError {
        InstallError::Store(error)
    }
}

impl From<SourceError> for InstallError {
    fn from(error: SourceError) -> InstallError {
        InstallError::Download(error)
    }
}

/// Why text is not a source, or a source could not be read or downloaded from.
#[derive(Debug)]
pub enum SourceError {
    /// The text is neither an index's URL nor a GitHub repository, for `reason`.
    NotASource { text: String, reason: &'static str },
    /// No answer came from `url`, in the HTTP client's words.
    Unreachable { url: Url, error: reqwest::Error },
    /// `url` answered with a status other than success.
    Status { url: Url, status: StatusCode },
    /// The index at `url` is larger than `limit` bytes, the most the store reads of one.
    IndexTooLarge { url: Url, limit: u64 },
    /// What `url` answered is not an index.
    NotAnIndex { url: Url, error: IndexError },
    /// The index lists a package whose place is not a URL.
    PackageUrl {
        package: String,
        error: url::ParseError,
    },
    /// The package downloaded is not the one the index lists.
    Sha256Mismatch {
        listed: [u8; 32],
        downloaded: [u8; 32],
    },
    /// Writing the download at `path` failed.
    Io { path: PathBuf, error: io::Error },
}

impl SourceError {
    /// For `map_err`: no answer came from `url`.
    pub(crate) fn unreachable_at(url: &Url) -> impl FnOnce(reqwest::Error) -> SourceError + '_ {
        move |error| SourceError::Unreachable {
            url: url.clone(),
            error,
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::NotASource { text, reason } => {
                write!(f, "{text:?} is not a package source: {reason}")
            }
            SourceError::Unreachable { url, error } => {
                write!(f, "no answer from {url}: {}", with_causes(error))
            }
            SourceError::Status { url, status } => write!(f, "{url} answered {status}"),
            SourceError::IndexTooLarge { url, limit } => write!(
                f,
                "the index at {url} is larger than {} MiB, the most WaOS reads of one",
                limit / (1024 * 1024)
            ),
            SourceError::NotAnIndex { url, error } => write!(f, "{url}: {error}"),
            SourceError::PackageUrl { package, error } => {
                write!(
                    f,
                    "the index lists the package {package:?}, which is not a URL: {error}"
                )
            }
            SourceError::Sha256Mismatch { listed, downloaded } => write!(
                f,
                "the package downloaded is not the one the index lists: its SHA-256 is {}, and the index lists {}",
                hex::encode(downloaded),
                hex::encode(listed)
            ),
            SourceError::Io { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

/// The causes are in each message already, so a chain of errors does not say them twice.
impl Error for SourceError {}

/// `error` and each error that caused it, joined as one sentence: the HTTP client's own message
/// names the request, and its causes say what went wrong.
fn with_causes(error: &dyn Error) -> String {
    let mut said = error.to_string();
    let mut cause = error.source();
    while let Some(error) = cause {
        said.push_str(": ");
        said.push_str(&error.to_string());
        cause = error.source();
    }
    said
}
