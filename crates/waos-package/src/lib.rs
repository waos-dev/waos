//! The `.wapk` format: how a program is distributed, and how a package source lists what it
//! offers.
//!
//! A package is a zip whose entries are all deflated. Three at fixed names are the program:
//! [`MANIFEST`], its identity as a [`Manifest`]; [`COMPONENT`], the program as a component; and
//! [`ICON`], a square PNG at least 256 pixels on a side. Two prove who published it, as Android's
//! first scheme and Java's signed JARs do: [`PUBLISHER_KEY`], the publisher's [`PublisherKey`], and
//! `signature`, made with the publisher's [`SigningKey`] over the SHA-256 of the three, by name.
//! Entries with other names are outside the signature and ignored, so a later version can add its
//! own.
//!
//! [`Package::open`] verifies the signature and reads the manifest and icon; [`Package::unpack`]
//! writes the program and the publisher's key into a directory, streaming the component to disk.
//! [`write`] makes and signs a package. An [`Index`] is a source's `index.json`: each program it
//! offers, with its package and that package's SHA-256.
//!
//! The invariants a later editor will break: nothing a package says is believed before its
//! signature is verified; reading never holds more of an entry in memory than its cap, whatever
//! size the archive claims for it; and what [`write`] makes, [`Package::open`] accepts, since both
//! check the same entries against the same rules. The crate is the format and nothing else: it
//! names no other crate of ours, and what is installed where is `waos_services::packages`'s.

mod category;
mod entries;
mod error;
mod file_association;
#[cfg(test)]
mod fixtures;
mod icon;
mod id;
mod index;
mod keys;
mod manifest;
mod package;
mod signed_message;
mod transfer;
mod write;

pub use category::{Category, UnknownCategory};
pub use entries::{COMPONENT, ICON, MANIFEST, PUBLISHER_KEY};
pub use error::PackageError;
pub use file_association::{FileAssociation, FileAssociationError};
pub use icon::MINIMUM_SIDE as ICON_MINIMUM_SIDE;
pub use id::{DEVELOPMENT_SUFFIX, IdError, ProgramId};
pub use index::{Index, IndexError, ListedProgram};
pub use keys::{KeyError, PublisherKey, SigningKey};
pub use manifest::{Manifest, ManifestError, ProgramKind};
pub use package::Package;
pub use semver::Version;
pub use write::write;
