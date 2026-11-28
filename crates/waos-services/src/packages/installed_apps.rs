//! What other domains follow of the package manager's record of what is installed
//! ([`InstalledApps`]): the name the user knows an app by; the apps being removed; and each removal
//! once it is out of the record, at which each follower forgets its part of the app.
//!
//! The invariant: the package manager's record is the truth of what exists; a follower keeps no
//! list of its own, and nothing but the record's removals tells it to forget an app.

use std::collections::BTreeSet;

use tokio::sync::{mpsc, watch};
use waos_package::ProgramId;

use super::PackageManager;

/// What a domain follows of the package manager's record, as [`PackageManager`] gives it, or a
/// test's stand-in.
pub trait InstalledApps: Send + Sync {
    /// The name the user knows the app `id` by; none for an app not installed.
    fn name(&self, id: &ProgramId) -> Option<String>;

    /// The apps being removed now, and each change of them from now on.
    fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>>;

    /// Hears the id of each app removed from now on, once it is out of the record.
    fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId>;

    /// Whether the app `id` is installed.
    fn is_installed(&self, id: &ProgramId) -> bool {
        self.name(id).is_some()
    }
}

impl InstalledApps for PackageManager {
    fn name(&self, id: &ProgramId) -> Option<String> {
        Some(self.installed_program(id)?.name())
    }

    fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        PackageManager::removing(self)
    }

    fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId> {
        PackageManager::removals(self)
    }
}

/// No package manager: nothing is installed or removed, as for an embedder that runs apps it does
/// not install.
pub struct NothingInstalled;

impl InstalledApps for NothingInstalled {
    fn name(&self, _id: &ProgramId) -> Option<String> {
        None
    }

    fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        watch::channel(BTreeSet::new()).1
    }

    fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId> {
        mpsc::unbounded_channel().1
    }
}
