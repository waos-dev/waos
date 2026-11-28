//! Permissions: what the user lets each app use, and the questions being asked of the user now.
//! One domain owns the answers the user gave, the settings they set and the asking, as Android's
//! permission service and iOS's `tccd` own both the answers and the dialog, while the engine only
//! checks, through the grants each run is handed ([`Permissions::of`]): a run that needs the
//! camera waits on its grants, which answer with the answer kept, or ask the user now through the
//! [`Prompts`] Permissions is handed, once however many wait, and keep the answer. The questions
//! asked now are one list ([`Permissions::asking`]).
//!
//! What the user chose is kept in a [`SettingsStore`], in Permissions' own values: WaOS's files
//! ([`SettingsFiles`]), or memory in a test ([`SettingsInMemory`]). Every app follows the global
//! settings until the user sets its own, as Chrome's site settings do. Removing an app follows the
//! package manager's record ([`InstalledApps`]): an app being removed takes no write, and one
//! removed is forgotten.
//!
//! The invariant a later editor will break: a grant is read at each check, never copied into a
//! run, so a change the user makes holds from the run's next check on; and nothing but
//! Permissions writes the settings, so no rule of theirs is kept by a caller.

mod asking;
mod error;
mod grants;
mod installed_apps;
mod prompts;
mod service;
mod settings;
mod settings_files;
mod settings_store;

pub use asking::{AppQuestion, AskedQuestion};
pub use error::SettingsError;
pub use installed_apps::{InstalledApps, NothingInstalled};
pub use prompts::{App, NoPrompts, Prompts};
pub use service::Permissions;
pub use settings::{AnswerWith, AppSettings, GlobalSettings, PerQuestion};
pub use settings_files::SettingsFiles;
pub use settings_store::{SettingsInMemory, SettingsStore};
/// What the settings are made of, as `waos-capabilities` names them.
pub use waos_capabilities::{
    Answer, AnswerFuture, Domain, DomainError, DomainRule, FileDialog, FileSystemGrant, Mount,
    NetworkGrant, ProgramGrants, Question, Role, Setting, SettingsClosedFuture, SimulatedSound,
    Switch,
};
/// What an app is known by, as `waos-package` names it.
pub use waos_package::ProgramId;
