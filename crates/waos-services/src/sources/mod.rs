//! Sources: the places that list apps, which the user added ([`Sources`]), and what is read from
//! them: a source's index, its pictures, and its packages' downloads, which the package manager
//! takes in. The list is kept in a [`SourcesStore`], in its own values: WaOS's file
//! ([`SourcesFile`]), or memory in a test ([`SourcesInMemory`]); the pictures in a folder of their
//! own, as read.
//!
//! The invariant a later editor will break: a download's bytes end in an error unless their
//! SHA-256 is the one its index lists, and a listing of a development id is refused before its
//! download, so the package manager installs only what a source listed.

mod error;
mod fetch;
mod service;
mod source;
mod sources_file;
mod sources_store;

pub use error::{SourceError, SourceListError};
pub use fetch::{Download, SourceProxy};
pub use service::Sources;
pub use source::Source;
pub use sources_file::SourcesFile;
pub use sources_store::{SourcesInMemory, SourcesStore};
/// The addresses a source's files are at, which [`Source::picture_url`] answers.
pub use url::Url;
