//! Why a package could not be opened, unpacked or written.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use zip::result::ZipError;

use crate::entries::{COMPONENT, ICON, MANIFEST, PUBLISHER_KEY, SIGNATURE, SIGNATURE_LENGTH};
use crate::icon::MINIMUM_SIDE;
use crate::{DEVELOPMENT_SUFFIX, KeyError, ManifestError, ProgramId};

/// Why a package could not be opened, unpacked or written, in words the person installing or
/// making it can act on.
#[derive(Debug)]
pub enum PackageError {
    /// Reading or writing the file at `path` failed.
    Io {
        path: PathBuf,
        error: io::Error,
    },
    /// The package is not a zip archive this crate can read, in the zip library's words.
    NotAZipArchive(String),
    /// The package has no entry of this name.
    MissingEntry(&'static str),
    /// The entry is larger than `limit` bytes, the most a package may hold of it.
    TooLarge {
        entry: &'static str,
        limit: u64,
    },
    /// The publisher's key is not an Ed25519 public key.
    PublisherKey(KeyError),
    /// The signature is not the 64 bytes of an Ed25519 signature.
    SignatureNotEd25519 {
        length: usize,
    },
    /// The signature does not verify with the package's publisher key over its entries: they were
    /// changed after it was signed, or it was signed with another key.
    SignatureDoesNotMatch,
    /// Unpacking read a component other than the one opening verified: the file changed on disk
    /// in between.
    ChangedSinceOpened,
    /// The manifest is not UTF-8 text.
    ManifestNotUtf8,
    /// The manifest is not a valid one.
    Manifest(ManifestError),
    /// The icon does not begin as a PNG does: the PNG signature, then an IHDR header.
    IconNotPng,
    IconNotSquare {
        width: u32,
        height: u32,
    },
    /// The icon is square and smaller than 256 pixels on a side.
    IconTooSmall {
        side: u32,
    },
    /// The id ends in `.development`, which names the development install of the program of the
    /// id before it, made from its source, and which no package may take.
    DevelopmentId(ProgramId),
}

impl PackageError {
    /// For `map_err`: an IO error at `path`.
    pub(crate) fn io_at(path: &Path) -> impl FnOnce(io::Error) -> PackageError + '_ {
        move |error| PackageError::Io {
            path: path.to_path_buf(),
            error,
        }
    }

    /// For `map_err` while reading the archive at `path`: its IO errors as IO errors there,
    /// and anything else as an archive this crate cannot read.
    pub(crate) fn reading_archive_at(path: &Path) -> impl FnOnce(ZipError) -> PackageError + '_ {
        move |error| match error {
            ZipError::Io(error) => PackageError::io_at(path)(error),
            other => PackageError::NotAZipArchive(other.to_string()),
        }
    }

    /// For `map_err` while writing the archive at `path`: every error as an IO error there.
    pub(crate) fn writing_archive_at(path: &Path) -> impl FnOnce(ZipError) -> PackageError + '_ {
        move |error| match error {
            ZipError::Io(error) => PackageError::io_at(path)(error),
            other => PackageError::io_at(path)(io::Error::other(other.to_string())),
        }
    }
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageError::Io { path, error } => write!(f, "{}: {error}", path.display()),
            PackageError::NotAZipArchive(reason) => {
                write!(
                    f,
                    "the package is not a zip archive this can read: {reason}"
                )
            }
            PackageError::MissingEntry(entry) => write!(f, "the package has no {entry}"),
            PackageError::TooLarge { entry, limit } => {
                write!(
                    f,
                    "{entry} is larger than {}, the most a package may hold",
                    in_units(*limit)
                )
            }
            PackageError::PublisherKey(error) => write!(f, "{PUBLISHER_KEY}: {error}"),
            PackageError::SignatureNotEd25519 { length } => write!(
                f,
                "{SIGNATURE} is {length} bytes; an Ed25519 signature is {SIGNATURE_LENGTH}"
            ),
            PackageError::SignatureDoesNotMatch => write!(
                f,
                "the package's {SIGNATURE} does not match its {MANIFEST}, {COMPONENT} and {ICON} with its {PUBLISHER_KEY}: they were changed after the package was signed, or it was signed with another key"
            ),
            PackageError::ChangedSinceOpened => write!(
                f,
                "the package's {COMPONENT} changed on disk after the package was opened"
            ),
            PackageError::ManifestNotUtf8 => write!(f, "{MANIFEST} is not UTF-8 text"),
            PackageError::Manifest(error) => write!(f, "{MANIFEST}: {error}"),
            PackageError::IconNotPng => write!(f, "{ICON} is not a PNG"),
            PackageError::IconNotSquare { width, height } => {
                write!(f, "{ICON} is {width}×{height} pixels; an icon is square")
            }
            PackageError::IconTooSmall { side } => write!(
                f,
                "{ICON} is {side}×{side} pixels; an icon is at least {MINIMUM_SIDE} pixels on a side"
            ),
            PackageError::DevelopmentId(id) => write!(
                f,
                "the id {id} ends in {DEVELOPMENT_SUFFIX}, which names a program's development install, made from its source by `waos install`; a package takes the program's own id"
            ),
        }
    }
}

/// The causes are in each message already, so a chain of errors does not say them twice.
impl std::error::Error for PackageError {}

impl From<ManifestError> for PackageError {
    fn from(error: ManifestError) -> PackageError {
        PackageError::Manifest(error)
    }
}

/// `bytes` in the largest binary unit that divides it.
fn in_units(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    match bytes {
        _ if bytes >= MIB && bytes.is_multiple_of(MIB) => format!("{} MiB", bytes / MIB),
        _ if bytes >= KIB && bytes.is_multiple_of(KIB) => format!("{} KiB", bytes / KIB),
        _ => format!("{bytes} bytes"),
    }
}
