//! WaOS's system services that need no engine, a module per domain, as Android keeps its system
//! services as packages of one `services` module, apart from its runtime: what is installed
//! ([`packages`]), what the user lets each app use ([`permissions`]), the places that list apps
//! ([`sources`]), what each app did ([`activity`]), and each app's own folders
//! ([`app_storage`]). Each domain owns one resource, is handed what it needs from outside when it
//! is made, and the WaOS app's startup puts them together; the app instances, what runs, stay in
//! the app, since they need the engine.
//!
//! Each domain keeps its files in WaOS's store directory, where they always were, which people
//! have; nothing on disk moves or changes format:
//!
//! ```text
//! <directory>/
//!   programs/
//!     dev.example.triangle/
//!       current, 1.2.0-Xk3q9a/   packages: what is installed
//!       settings.toml            permissions: what the user lets the app use
//!       data/  cache/            app storage: the app's own folders, its /data and /cache
//!   removing/                    packages: an app being removed, moved here whole first
//!   audit/
//!     dev.example.triangle/
//!       2026-11-15.postcard      the activity record: what the app did that day
//!   pictures/                    sources: their icons and screenshots, kept as read
//!   sources.toml                 sources: the sources the user added, in the order added
//!   settings.toml                permissions: the global settings
//! ```
//!
//! The invariant a later editor will break: module privacy is the wall between domains. A
//! domain's API is what its module makes `pub`, its insides sit in private submodules, and a
//! domain reaches another only through that other's public API or a small trait it is handed;
//! no `pub(crate)` item is there for another domain to reach.

pub mod activity;
pub mod app_storage;
mod files;
pub mod packages;
pub mod permissions;
pub mod sources;
