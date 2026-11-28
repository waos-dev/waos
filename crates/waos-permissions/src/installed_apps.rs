//! What Permissions follows of the package manager's record of what is installed
//! ([`InstalledApps`]): the apps being removed, whose settings take no write, and each removal once
//! it is out of the record, whose settings Permissions then forgets.
//!
//! The invariant: the package manager's record is the truth of what exists; Permissions follows
//! it and keeps no list of its own.

use std::collections::BTreeSet;

use tokio::sync::{mpsc, watch};
use waos_package::ProgramId;

/// What Permissions follows of the package manager: `waos-package-manager`'s `PackageManager`,
/// through a type of the embedder's own, since neither crate knows the other.
pub trait InstalledApps: Send + Sync {
    /// The apps being removed now, and each change of them from now on.
    fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>>;

    /// Hears the id of each app removed from now on, once it is out of the record.
    fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId>;
}

/// No package manager: nothing is removed, as for an embedder that runs apps it does not install.
pub struct NothingInstalled;

impl InstalledApps for NothingInstalled {
    fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        watch::channel(BTreeSet::new()).1
    }

    fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId> {
        mpsc::unbounded_channel().1
    }
}
