//! The package sources the user added and their downloads, each app's own folders, and what each
//! app did, kept in a directory the app gives the [`Store`]. What is installed is the package
//! manager's (`waos_services::packages`), and what the user lets each app use is Permissions'
//! (`waos_services::permissions`), both in the same directory. An app bundling one program builds none of
//! it.
//!
//! ```text
//! <directory>/
//!   programs/
//!     dev.example.triangle/
//!       current, 1.2.0-Xk3q9a/   the package manager's: what is installed
//!       settings.toml     Permissions': what the user lets the app use
//!       data/             the app's own folders, which it sees at /data and /cache
//!       cache/
//!   audit/
//!     dev.example.triangle/
//!       2026-11-15.postcard  what the app did that day, its last seven days kept
//!   pictures/             the sources' icons and screenshots, kept as read, by their addresses
//!   sources.toml          the sources the user added, in the order added
//!   settings.toml         Permissions': the global settings
//! ```
//!
//! An app's own folders are made at its first run, and an install of its id keeps them. The
//! package manager removes an app's folder whole, its own folders with it; the store then forgets
//! its activity ([`Store::forget`]), one file a day, kept seven days, on this device only
//! (`audit`, the engine's `AuditLog`), recorded as the app's settings say.
//!
//! The invariant a later editor will break: a download's bytes end in an error unless their
//! SHA-256 is the one its index lists, and a listing of a development id is refused before its
//! download, so the package manager installs only what a source listed.

mod audit;
mod error;
mod fetch;
mod files;
mod programs;
mod source;
mod sources_file;
mod store;

pub use error::{SourceError, StoreError};
pub use fetch::{Download, SourceProxy};
pub use source::Source;
pub use store::Store;
/// The addresses a source's files are at, which [`Source::picture_url`] answers.
pub use url::Url;
