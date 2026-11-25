//! The programs installed in WaOS, their updates, and the package sources they come from, kept in
//! a directory the app gives the [`Store`]. An app bundling one program builds none of it.
//!
//! ```text
//! <directory>/
//!   programs/
//!     dev.example.triangle/
//!       current           the name of the folder below that is installed
//!       1.2.0-Xk3q9a/     what the package unpacked to: manifest.toml, program.wasm, icon.png, publisher.key
//!       settings.toml     the program's own settings: what the user lets it use
//!       data/             the program's own folders, which it sees at /data and /cache
//!       cache/
//!     dev.example.triangle.development/   its development install, a program of its own
//!   audit/
//!     dev.example.triangle/
//!       2026-11-15.postcard  what the program did that day, its last seven days kept
//!   pictures/             the sources' icons and screenshots, kept as read, by their addresses
//!   removing/             a program being removed, moved here whole first
//!   sources.toml          the sources the user added, in the order added
//!   settings.toml         the global settings, which every program follows until it has its own
//! ```
//!
//! What the user lets each program use is its settings, in its folder, over the global ones,
//! which the store answers to the engine as each program's `ProgramGrants`, read live
//! (decisions 7 and 8 of the permissions proposal). A program run in development is the one its
//! id names (decision 11): its folder is made at its first run, installed or not, with its own
//! folders and settings, and an install of its id keeps them. A program's folder goes with it,
//! whole, when it is removed, and its audit summary too: what it did with what it was allowed,
//! one file a day, kept seven days, on this device only (`audit`, the engine's `AuditLog`).
//!
//! An installed program is its manifest and where it came from, its [`Origin`]: a package, with
//! its publisher's key and no record of which source listed it (the design's decision 46), which
//! installs as a new id, or as an update of an installed id when it is signed with the same key and
//! is newer, any source listing an installed id at a newer version offering an update; or its
//! source, a development install, which `waos install` makes under the program's development id
//! (`dev.example.triangle.development`), a program of its own beside the one published under the
//! plain id, which no package or source replaces.
//!
//! The invariants a later editor will break: the store believes a package only once
//! `waos-package` has verified it, and installs a development build under its development id
//! alone, and a download only once its SHA-256 is the one its index
//! lists; and replacing a program's `current` is the one step that changes what is installed,
//! and moving its folder into `removing/` the one that removes it, so a crash leaves the earlier
//! install or the new one, the program or none of it, never part of either.

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

pub use error::{InstallError, SourceError, StoreError};
pub use fetch::SourceProxy;
pub use settings::{AnswerWith, GlobalSettings, PerQuestion, ProgramSettings};
pub use source::Source;
pub use store::{DevelopmentBuild, Download, InstalledProgram, Origin, Store};
/// The addresses a source's files are at, which [`Source::picture_url`] answers.
pub use url::Url;
/// What the settings are made of, as `waos-capabilities` names them.
pub use waos_capabilities::{
    Answer, Domain, DomainError, DomainRule, FileDialog, FileSystemGrant, Mount, NetworkGrant,
    Question, Role, Setting,
};
