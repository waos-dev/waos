//! The package sources the user added and their downloads, what the user lets each app use, each
//! app's own folders, and what each app did, kept in a directory the app gives the [`Store`]. What
//! is installed is the package manager's (`waos-package-manager`), in the same directory. An app
//! bundling one program builds none of it.
//!
//! ```text
//! <directory>/
//!   programs/
//!     dev.example.triangle/
//!       current, 1.2.0-Xk3q9a/   the package manager's: what is installed
//!       settings.toml     the app's own settings: what the user lets it use
//!       data/             the app's own folders, which it sees at /data and /cache
//!       cache/
//!   audit/
//!     dev.example.triangle/
//!       2026-11-15.postcard  what the app did that day, its last seven days kept
//!   pictures/             the sources' icons and screenshots, kept as read, by their addresses
//!   sources.toml          the sources the user added, in the order added
//!   settings.toml         the global settings, which every app follows until it has its own
//! ```
//!
//! What the user lets each app use is its settings, in its folder, over the global ones, which the
//! store answers to the engine as each app's `ProgramGrants`, read live. An app run in development
//! is the one its id names: its folder is made at its first run, installed or not, with its own
//! folders and settings, and an install of its id keeps them. The package manager removes an
//! app's folder whole, its settings and own folders with it; the store then forgets what it holds
//! of the app ([`Store::forget`]): its settings in memory and its activity, one file a day, kept
//! seven days, on this device only (`audit`, the engine's `AuditLog`).
//!
//! The invariant a later editor will break: a download's bytes end in an error unless their
//! SHA-256 is the one its index lists, and a listing of a development id is refused before its
//! download, so the package manager installs only what a source listed.

mod audit;
mod error;
mod fetch;
mod files;
mod grants;
mod programs;
mod settings;
mod settings_file;
mod source;
mod sources_file;
mod store;

pub use error::{SourceError, StoreError};
pub use fetch::{Download, SourceProxy};
pub use settings::{AnswerWith, GlobalSettings, PerQuestion, ProgramSettings};
pub use source::Source;
pub use store::Store;
/// The addresses a source's files are at, which [`Source::picture_url`] answers.
pub use url::Url;
/// What the settings are made of, as `waos-capabilities` names them.
pub use waos_capabilities::{
    Answer, Domain, DomainError, DomainRule, FileDialog, FileSystemGrant, Mount, NetworkGrant,
    Question, Role, Setting,
};
