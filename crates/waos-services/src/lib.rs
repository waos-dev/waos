//! WaOS's system services that need no engine, a module per domain, as Android keeps its system
//! services as packages of one `services` module, apart from its runtime: what is installed
//! ([`packages`]), what the user lets each app use ([`permissions`]), the places that list apps
//! ([`sources`]), each app's own folders ([`app_storage`]), and the store ([`store`]): each app's
//! activity. Each domain owns one resource, is handed
//! what it needs from outside when it is made, and the WaOS app's startup puts them together; the
//! running apps and the app starter stay in the app, since they need the engine.
//!
//! The invariant a later editor will break: module privacy is the wall between domains. A
//! domain's API is what its module makes `pub`, its insides sit in private submodules, and a
//! domain reaches another only through that other's public API or a small trait it is handed;
//! no `pub(crate)` item is there for another domain to reach.

pub mod app_storage;
mod files;
pub mod packages;
pub mod permissions;
pub mod sources;
pub mod store;
