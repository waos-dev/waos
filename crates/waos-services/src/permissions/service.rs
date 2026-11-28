//! [`Permissions`]: what the user lets each app use, held in memory over its store, so a check
//! reads no file, and written through to the store under one lock, each change heard by every
//! [`Permissions::changed`] asked for before it; and the questions being asked of the user now
//! (`asking`), through the prompts it is handed. An app's settings are kept whether or not it is
//! installed: `waos settings set` answers ahead for an id before its install, so nothing is
//! forgotten but what the package manager removes.
//!
//! The invariants: two changes never interleave, as every change holds the one lock from its read
//! to its write; an app being removed takes no write, and an app the package manager removed is
//! forgotten before the next read or change, so no setting outlives its app's removal to be found
//! by a reinstall.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, mpsc, watch};
use waos_capabilities::{ChangedFuture, ProgramGrants};
use waos_package::ProgramId;

use crate::packages::InstalledApps;
use crate::permissions::asking::Asking;
use crate::permissions::grants::AppGrants;
use crate::permissions::settings::{AppSettings, GlobalSettings};
use crate::permissions::settings_store::locked;
use crate::permissions::{AskedQuestion, Prompts, SettingsError, SettingsStore};

/// What the user lets each app use. Clones are the same.
#[derive(Clone)]
pub struct Permissions {
    shared: Arc<Shared>,
}

/// What a [`Permissions`] and its apps' grants share.
pub(crate) struct Shared {
    store: Arc<dyn SettingsStore>,
    /// What asks the user.
    pub(crate) prompts: Arc<dyn Prompts>,
    /// What says the name of an app, and its removals.
    pub(crate) installed: Arc<dyn InstalledApps>,
    /// The questions being asked now, by what each asks.
    pub(crate) asking: Mutex<HashMap<AskedQuestion, Asking>>,
    /// The questions being asked now, in the order asked, for whoever shows or reports them.
    pub(crate) asked: watch::Sender<Vec<AskedQuestion>>,
    /// Tells one asking from the next.
    pub(crate) serials: AtomicU64,
    /// The apps the package manager is removing now.
    removing: watch::Receiver<BTreeSet<ProgramId>>,
    /// Each app the package manager removed, not yet forgotten.
    removals: Mutex<mpsc::UnboundedReceiver<ProgramId>>,
    /// The settings read so far, held for one read or one change at a time.
    held: Mutex<Held>,
    changed: Arc<Notify>,
}

/// The settings read from the store so far.
#[derive(Default)]
struct Held {
    global: Option<GlobalSettings>,
    apps: HashMap<ProgramId, AppSettings>,
}

impl Permissions {
    /// What the user lets each app use, kept in `store`, asked of the user through `prompts`,
    /// following what `installed` says of the apps installed.
    pub fn open(
        store: Arc<dyn SettingsStore>,
        prompts: Arc<dyn Prompts>,
        installed: Arc<dyn InstalledApps>,
    ) -> Permissions {
        Permissions {
            shared: Arc::new(Shared {
                store,
                prompts,
                removing: installed.removing(),
                removals: Mutex::new(installed.removals()),
                installed,
                asking: Mutex::default(),
                asked: watch::Sender::default(),
                serials: AtomicU64::new(0),
                held: Mutex::default(),
                changed: Arc::default(),
            }),
        }
    }

    /// One app's permissions, as its run is handed them: read live, so a change the user makes
    /// holds from the run's next check.
    pub fn of(&self, app: &ProgramId) -> Arc<dyn ProgramGrants> {
        Arc::new(AppGrants {
            shared: Arc::clone(&self.shared),
            app: app.clone(),
        })
    }

    /// The settings every app follows until the user sets its own.
    pub fn global_settings(&self) -> Result<GlobalSettings, SettingsError> {
        self.shared.global_settings()
    }

    /// Changes the global settings as `change` does; every app without its own hears it.
    pub fn change_global_settings(
        &self,
        change: impl FnOnce(&mut GlobalSettings),
    ) -> Result<(), SettingsError> {
        self.shared.change_global(change)
    }

    /// The app's own settings, each the global one until the user sets it: those of an app
    /// installed, run, or set before either.
    pub fn app_settings(&self, app: &ProgramId) -> Result<AppSettings, SettingsError> {
        self.shared.app_settings(app)
    }

    /// Changes the app's own settings as `change` does, installed or not yet; the app hears it. An
    /// app being removed is refused.
    pub fn change_app_settings(
        &self,
        app: &ProgramId,
        change: impl FnOnce(&mut AppSettings),
    ) -> Result<(), SettingsError> {
        self.shared.change_app(app, change)
    }

    /// Resolves at the next change of any settings, the global ones or an app's own.
    pub fn changed(&self) -> ChangedFuture {
        self.shared.changed()
    }

    /// The questions being asked of the user now, in the order asked, and each change of them
    /// from now on, for whoever shows or reports them.
    pub fn asking(&self) -> watch::Receiver<Vec<AskedQuestion>> {
        self.shared.asked.subscribe()
    }
}

impl Shared {
    pub(crate) fn global_settings(&self) -> Result<GlobalSettings, SettingsError> {
        let mut held = self.held();
        self.global_in(&mut held)
    }

    pub(crate) fn app_settings(&self, app: &ProgramId) -> Result<AppSettings, SettingsError> {
        let mut held = self.held();
        self.app_in(&mut held, app)
    }

    /// The global settings and the app's own.
    pub(crate) fn both(
        &self,
        app: &ProgramId,
    ) -> Result<(GlobalSettings, AppSettings), SettingsError> {
        let mut held = self.held();
        Ok((self.global_in(&mut held)?, self.app_in(&mut held, app)?))
    }

    pub(crate) fn change_global(
        &self,
        change: impl FnOnce(&mut GlobalSettings),
    ) -> Result<(), SettingsError> {
        let mut held = self.held();
        let mut settings = self.global_in(&mut held)?;
        change(&mut settings);
        self.store.save_global(&settings)?;
        held.global = Some(settings);
        self.changed.notify_waiters();
        Ok(())
    }

    /// Changes the app's own settings as `change` does, given the global ones and its own.
    pub(crate) fn change_app_with_global(
        &self,
        app: &ProgramId,
        change: impl FnOnce(&GlobalSettings, &mut AppSettings),
    ) -> Result<(), SettingsError> {
        let mut held = self.held();
        if self.removing.borrow().contains(app) {
            return Err(SettingsError::Removing(app.clone()));
        }
        let global = self.global_in(&mut held)?;
        let mut settings = self.app_in(&mut held, app)?;
        change(&global, &mut settings);
        self.store.save(app, &settings)?;
        held.apps.insert(app.clone(), settings);
        self.changed.notify_waiters();
        Ok(())
    }

    pub(crate) fn change_app(
        &self,
        app: &ProgramId,
        change: impl FnOnce(&mut AppSettings),
    ) -> Result<(), SettingsError> {
        self.change_app_with_global(app, |_global, settings| change(settings))
    }

    pub(crate) fn changed(&self) -> ChangedFuture {
        Box::pin(Arc::clone(&self.changed).notified_owned())
    }

    /// The settings held, once the removals the package manager told since are forgotten.
    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        let mut held = locked(&self.held);
        let mut removals = locked(&self.removals);
        let mut forgot = false;
        while let Ok(app) = removals.try_recv() {
            held.apps.remove(&app);
            if let Err(error) = self.store.forget(&app) {
                tracing::warn!(%app, "a removed app's settings are left: {error}");
            }
            forgot = true;
        }
        if forgot {
            self.changed.notify_waiters();
        }
        held
    }

    fn global_in(&self, held: &mut Held) -> Result<GlobalSettings, SettingsError> {
        if let Some(settings) = &held.global {
            return Ok(settings.clone());
        }
        let settings = self.store.load_global()?;
        held.global = Some(settings.clone());
        Ok(settings)
    }

    fn app_in(&self, held: &mut Held, app: &ProgramId) -> Result<AppSettings, SettingsError> {
        if let Some(settings) = held.apps.get(app) {
            return Ok(settings.clone());
        }
        let settings = self.store.load(app)?.unwrap_or_default();
        held.apps.insert(app.clone(), settings.clone());
        Ok(settings)
    }
}
