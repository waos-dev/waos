//! Where the activity record keeps the apps' days, in its own values ([`ActivityStore`]): files
//! today (`activity_files`), memory in a test ([`ActivityInMemory`]).
//!
//! The invariant: a store keeps what it is handed and nothing else; every rule of the record
//! (which apps, how many days, whether at all) is the record's.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use waos_capabilities::AuditDay;
use waos_package::ProgramId;

/// Where the apps' days are kept, one per app and day.
pub trait ActivityStore: Send + Sync {
    /// The days kept of `app`, in no order.
    fn days(&self, app: &ProgramId) -> io::Result<Vec<AuditDay>>;

    /// Keeps `day` of `app`, in place of what was kept of the same day.
    fn keep(&self, app: &ProgramId, day: &AuditDay) -> io::Result<()>;

    /// Removes the day of `app` that starts at `start`; a day not kept is nothing to remove.
    fn remove(&self, app: &ProgramId, start: SystemTime) -> io::Result<()>;

    /// Removes every day kept of `app`.
    fn forget(&self, app: &ProgramId) -> io::Result<()>;

    /// The apps a day is kept of.
    fn apps(&self) -> io::Result<Vec<ProgramId>>;
}

/// Days kept in memory alone, gone with the value: for tests.
#[derive(Default)]
pub struct ActivityInMemory {
    days: Mutex<BTreeMap<ProgramId, BTreeMap<SystemTime, AuditDay>>>,
}

impl ActivityInMemory {
    fn locked(&self) -> MutexGuard<'_, BTreeMap<ProgramId, BTreeMap<SystemTime, AuditDay>>> {
        self.days.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ActivityStore for ActivityInMemory {
    fn days(&self, app: &ProgramId) -> io::Result<Vec<AuditDay>> {
        let days = self.locked();
        Ok(days
            .get(app)
            .into_iter()
            .flat_map(|days| days.values().cloned())
            .collect())
    }

    fn keep(&self, app: &ProgramId, day: &AuditDay) -> io::Result<()> {
        let mut days = self.locked();
        days.entry(app.clone())
            .or_default()
            .insert(day.start, day.clone());
        Ok(())
    }

    fn remove(&self, app: &ProgramId, start: SystemTime) -> io::Result<()> {
        let mut days = self.locked();
        if let Some(kept) = days.get_mut(app) {
            kept.remove(&start);
            if kept.is_empty() {
                days.remove(app);
            }
        }
        Ok(())
    }

    fn forget(&self, app: &ProgramId) -> io::Result<()> {
        self.locked().remove(app);
        Ok(())
    }

    fn apps(&self) -> io::Result<Vec<ProgramId>> {
        Ok(self.locked().keys().cloned().collect())
    }
}
