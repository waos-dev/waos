//! Where Permissions keeps what the user chose, in its own values ([`SettingsStore`]): files
//! today (`settings_files`), the user's account later, memory in a test ([`SettingsInMemory`]).
//!
//! The invariant: a store keeps what it is handed and nothing else; every rule of the settings
//! (what is refused, what is forgotten, who hears a change) is Permissions'.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Mutex, MutexGuard, PoisonError};

use waos_package::ProgramId;

use crate::settings::{AppSettings, GlobalSettings};

/// Where Permissions keeps what the user chose: each app's own settings, and the global ones.
pub trait SettingsStore: Send + Sync {
    /// The app's own settings; none where nothing is kept for it.
    fn load(&self, app: &ProgramId) -> io::Result<Option<AppSettings>>;

    /// Keeps `settings` as the app's own, in place of what was kept.
    fn save(&self, app: &ProgramId, settings: &AppSettings) -> io::Result<()>;

    /// Removes what is kept for the app; nothing kept is nothing to remove.
    fn forget(&self, app: &ProgramId) -> io::Result<()>;

    /// The apps settings are kept for.
    fn apps(&self) -> io::Result<Vec<ProgramId>>;

    /// The global settings: the defaults where none are kept.
    fn load_global(&self) -> io::Result<GlobalSettings>;

    /// Keeps `settings` as the global ones.
    fn save_global(&self, settings: &GlobalSettings) -> io::Result<()>;
}

/// Settings kept in memory alone, gone with the value: for tests, and an embedder with nothing to
/// keep them in.
#[derive(Default)]
pub struct SettingsInMemory {
    apps: Mutex<BTreeMap<ProgramId, AppSettings>>,
    global: Mutex<GlobalSettings>,
}

impl SettingsStore for SettingsInMemory {
    fn load(&self, app: &ProgramId) -> io::Result<Option<AppSettings>> {
        Ok(locked(&self.apps).get(app).cloned())
    }

    fn save(&self, app: &ProgramId, settings: &AppSettings) -> io::Result<()> {
        locked(&self.apps).insert(app.clone(), settings.clone());
        Ok(())
    }

    fn forget(&self, app: &ProgramId) -> io::Result<()> {
        locked(&self.apps).remove(app);
        Ok(())
    }

    fn apps(&self) -> io::Result<Vec<ProgramId>> {
        Ok(locked(&self.apps).keys().cloned().collect())
    }

    fn load_global(&self) -> io::Result<GlobalSettings> {
        Ok(locked(&self.global).clone())
    }

    fn save_global(&self, settings: &GlobalSettings) -> io::Result<()> {
        *locked(&self.global) = settings.clone();
        Ok(())
    }
}

/// A lock held for one read or write: a panic while it was held left the value as it was before
/// or after that one write.
pub(crate) fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
