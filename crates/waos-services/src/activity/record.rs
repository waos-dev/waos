//! [`ActivityRecord`]: the days kept of what each app did, in the store it is handed, kept only of
//! an app the package manager's record holds and whose user keeps a record of it, and its last
//! [`DAYS_KEPT`] alone. What was kept of an app goes once its record is turned off, by its own
//! setting or the global one, or once the app is removed, on a thread of the record's own that
//! follows the settings and the removals; until then, the record answers nothing of it.
//!
//! The invariant: a day is kept, read or forgotten under one lock, and kept only for an app that
//! is installed and recorded at that moment, so a run of an app that ends after its removal, or
//! after its record is turned off, never keeps it again.

use std::io;
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use futures::future::{self, Either};
use tokio::sync::mpsc;
use tracing::warn;
use waos_capabilities::{AuditDay, AuditLog, ChangedFuture, DAYS_KEPT};
use waos_package::ProgramId;

use crate::activity::ActivityStore;
use crate::packages::InstalledApps;
use crate::permissions::Permissions;

/// A day.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// What each app did, its last days. Clones are the same.
#[derive(Clone)]
pub struct ActivityRecord {
    shared: Arc<Shared>,
}

struct Shared {
    store: Arc<dyn ActivityStore>,
    /// Whether the user keeps a record of each app.
    permissions: Permissions,
    /// The package manager's record: which apps are installed.
    installed: Arc<dyn InstalledApps>,
    /// Each app the package manager removed, not yet forgotten; held while a day is kept, read or
    /// forgotten, so changes happen one at a time.
    removals: Mutex<mpsc::UnboundedReceiver<ProgramId>>,
}

impl ActivityRecord {
    /// The days kept in `store`, recorded as `permissions` say, of the apps `installed` holds;
    /// what is kept of an app neither holds is removed now, and from now on as either changes.
    pub fn open(
        store: Arc<dyn ActivityStore>,
        permissions: Permissions,
        installed: Arc<dyn InstalledApps>,
    ) -> io::Result<ActivityRecord> {
        let (removals, followed_removals) = (installed.removals(), installed.removals());
        let record = ActivityRecord {
            shared: Arc::new(Shared {
                store,
                permissions,
                installed,
                removals: Mutex::new(removals),
            }),
        };
        // Asked for before the sweep, so a change during it is heard.
        let changed = record.shared.permissions.changed();
        record.shared.forget_unrecorded();
        follow(Arc::downgrade(&record.shared), changed, followed_removals)?;
        Ok(record)
    }

    /// The apps a day is kept of, by id.
    pub fn apps(&self) -> Vec<ProgramId> {
        let _changing = self.shared.lock_changes();
        let mut apps = self.shared.store.apps().unwrap_or_else(|error| {
            warn!(%error, "the apps' days cannot be listed");
            Vec::new()
        });
        apps.retain(|app| self.shared.records(app));
        apps.sort();
        apps
    }
}

impl AuditLog for ActivityRecord {
    /// Keeps `day` of `program`, an app's id, and removes its days too old to keep; nothing for
    /// a name that is no installed app's, or an app whose record is off.
    fn keep(&self, program: &str, day: &AuditDay) {
        let Ok(app) = program.parse::<ProgramId>() else {
            return;
        };
        let _changing = self.shared.lock_changes();
        if !self.shared.records(&app) {
            return;
        }
        let kept = (self.shared.store.keep(&app, day)).and_then(|()| self.shared.trim(&app, day));
        if let Err(error) = kept {
            warn!(%app, %error, "an app's day is not kept");
        }
    }

    fn days(&self, program: &str) -> Vec<AuditDay> {
        let Ok(app) = program.parse::<ProgramId>() else {
            return Vec::new();
        };
        let _changing = self.shared.lock_changes();
        if !self.shared.records(&app) {
            return Vec::new();
        }
        let mut days = self.shared.store.days(&app).unwrap_or_else(|error| {
            warn!(%app, %error, "an app's days cannot be read");
            Vec::new()
        });
        days.sort_by_key(|day| std::cmp::Reverse(day.start));
        days.truncate(DAYS_KEPT);
        days
    }
}

impl Shared {
    /// Whether days are kept of `app`: it is installed, and its user keeps a record of it.
    fn records(&self, app: &ProgramId) -> bool {
        self.installed.is_installed(app) && self.permissions.of(app).keeps_record()
    }

    /// Removes the days of `app` older than the [`DAYS_KEPT`] that end with `newest`.
    fn trim(&self, app: &ProgramId, newest: &AuditDay) -> io::Result<()> {
        let oldest = newest.start - DAY * (DAYS_KEPT as u32 - 1);
        for day in self.store.days(app)? {
            if day.start < oldest {
                self.store.remove(app, day.start)?;
            }
        }
        Ok(())
    }

    /// Removes what is kept of every app the record keeps no days of now.
    fn forget_unrecorded(&self) {
        let _changing = self.lock_changes();
        let Ok(apps) = self.store.apps() else {
            return;
        };
        for app in apps.iter().filter(|app| !self.records(app)) {
            if let Err(error) = self.store.forget(app) {
                warn!(%app, %error, "an app's days are left");
            }
        }
    }

    /// The lock every change holds, once the removals the package manager told since are
    /// forgotten, so no day kept before an app's removal outlives it to be found by a reinstall.
    fn lock_changes(&self) -> MutexGuard<'_, mpsc::UnboundedReceiver<ProgramId>> {
        // The receiver is whole whatever a panic interrupted, so its lock is taken back.
        let mut removals = self.removals.lock().unwrap_or_else(PoisonError::into_inner);
        while let Ok(app) = removals.try_recv() {
            if let Err(error) = self.store.forget(&app) {
                warn!(%app, %error, "a removed app's days are left");
            }
        }
        removals
    }
}

/// Removes what is kept of each app the record keeps no days of after `changed`, the next change
/// of the settings, or the next of `removals`, and so on after each, on a thread that ends with
/// the record, so what the record no longer keeps goes from the store without waiting for its
/// next use.
fn follow(
    followed: Weak<Shared>,
    mut changed: ChangedFuture,
    removals: mpsc::UnboundedReceiver<ProgramId>,
) -> io::Result<()> {
    let mut removals = Some(removals);
    std::thread::Builder::new()
        .name("activity".to_owned())
        .spawn(move || {
            loop {
                let removals_ended = futures::executor::block_on(async {
                    let Some(heard) = removals.as_mut() else {
                        (&mut changed).await;
                        return false;
                    };
                    let removal = pin!(heard.recv());
                    matches!(
                        future::select(&mut changed, removal).await,
                        Either::Right((None, _))
                    )
                });
                if removals_ended {
                    // The package manager is gone: only the settings change from now on.
                    removals = None;
                }
                let Some(shared) = followed.upgrade() else {
                    return;
                };
                // Asked for before the sweep, so a change during it is heard.
                changed = shared.permissions.changed();
                shared.forget_unrecorded();
            }
        })
        .map(drop)
}
