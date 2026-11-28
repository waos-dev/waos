//! Why a source could not be read, or the list of sources changed, in words the person using the
//! library can act on.

use std::error::Error;
use std::fmt;
use std::io;

use reqwest::StatusCode;
use url::Url;
use waos_package::{IndexError, ProgramId};

use crate::sources::Source;

/// Why the list of sources could not be read or changed.
#[derive(Debug)]
pub enum SourceListError {
    /// The store could not read or keep the list, in its words.
    Store(io::Error),
    /// The source is not among the sources added.
    NotAdded(Source),
}

impl fmt::Display for SourceListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceListError::Store(error) => error.fmt(f),
            SourceListError::NotAdded(source) => write!(f, "{source} is not one of the sources"),
        }
    }
}

impl Error for SourceListError {}

impl From<io::Error> for SourceListError {
    fn from(error: io::Error) -> SourceListError {
        SourceListError::Store(error)
    }
}

/// Why text is not a source, or a source could not be read or downloaded from.
#[derive(Debug)]
pub enum SourceError {
    /// The text is neither a website's or a folder's address nor a GitHub repository, for
    /// `reason`.
    NotASource { text: String, reason: &'static str },
    /// No answer came from `url`, in the HTTP client's words.
    Unreachable { url: Url, error: reqwest::Error },
    /// `url` answered with a status other than success.
    Status { url: Url, status: StatusCode },
    /// Nothing is at `url`, where the source's index would be: the website, the folder or the
    /// repository's latest release is no source.
    ListsNothing { url: Url },
    /// The index at `url` is larger than `limit` bytes, the most WaOS reads of one.
    IndexTooLarge { url: Url, limit: u64 },
    /// The index lists a picture at `url`, which is on another server than the index: a
    /// source's pictures are read from where its index is, and nowhere else.
    PictureElsewhere { url: Url },
    /// The picture at `url` is larger than `limit` bytes, the most WaOS reads of one.
    PictureTooLarge { url: Url, limit: u64 },
    /// What `url` answered is not an index.
    NotAnIndex { url: Url, error: IndexError },
    /// The index lists a package whose place is not a URL.
    PackageUrl {
        package: String,
        error: url::ParseError,
    },
    /// The index lists a package of a development id, which no package may take.
    DevelopmentId(ProgramId),
    /// The package downloaded is not the one the index lists.
    Sha256Mismatch {
        listed: [u8; 32],
        downloaded: [u8; 32],
    },
    /// The HTTP client sources are read with could not start, in its words.
    Http(reqwest::Error),
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
            SourceError::ListsNothing { url } => {
                write!(f, "it lists no apps for WaOS: nothing is at {url}")
            }
            SourceError::IndexTooLarge { url, limit } => write!(
                f,
                "the index at {url} is larger than {} MiB, the most WaOS reads of one",
                limit / (1024 * 1024)
            ),
            SourceError::PictureElsewhere { url } => write!(
                f,
                "the index lists a picture at {url}, on another server than the index, which WaOS does not read"
            ),
            SourceError::PictureTooLarge { url, limit } => write!(
                f,
                "the picture at {url} is larger than {} MiB, the most WaOS reads of one",
                limit / (1024 * 1024)
            ),
            SourceError::NotAnIndex { url, error } => write!(f, "{url}: {error}"),
            SourceError::PackageUrl { package, error } => {
                write!(
                    f,
                    "the index lists the package {package:?}, which is not a URL: {error}"
                )
            }
            SourceError::DevelopmentId(id) => write!(
                f,
                "the source lists {id}, a development install's id, which no package may take"
            ),
            SourceError::Sha256Mismatch { listed, downloaded } => write!(
                f,
                "the package downloaded is not the one the index lists: its SHA-256 is {}, and the index lists {}",
                hex::encode(downloaded),
                hex::encode(listed)
            ),
            SourceError::Http(error) => {
                write!(f, "the HTTP client could not start: {}", with_causes(error))
            }
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
