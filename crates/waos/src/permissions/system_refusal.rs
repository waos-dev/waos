//! Whether the system refuses a program what it guards itself, beside what the program's user
//! granted it in WaOS: its notifications, where each program is an app of its own with the system,
//! as on macOS, which asks for the app once and keeps the answer in its own settings. It is set
//! only by what the system says: its answer to a request for the program's notifications
//! (`ShellSystemPermission::request`), and what the shell reports whenever else it learns it
//! (`RunningProgram::system_notifications_refused`); the program's permission reads it.
//!
//! The invariant: a refusal is the system's alone and never written into the user's grants, so the
//! user's answer in WaOS is kept whatever the system says; nothing derives it from anything the
//! system did not say of its permission; and every change is heard by every stream of the
//! permission's changes.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::watch;

/// Whether the system refuses a program's notifications, as it last said; it lets them until it
/// says otherwise. Every clone is the same: the program's handle holds one, which the shell reports
/// into, and its permissions another.
#[derive(Clone)]
pub(crate) struct SystemRefusal(Arc<watch::Sender<bool>>);

impl Default for SystemRefusal {
    fn default() -> SystemRefusal {
        SystemRefusal(Arc::new(watch::Sender::new(false)))
    }
}

impl SystemRefusal {
    pub(crate) fn refuses(&self) -> bool {
        *self.0.borrow()
    }

    /// The system said whether it `refuses`: what waits for a change hears it, if it is one.
    pub(crate) fn set(&self, refuses: bool) {
        self.0.send_if_modified(|kept| {
            let changed = *kept != refuses;
            *kept = refuses;
            changed
        });
    }

    /// Resolves at the next change from now on.
    pub(crate) fn changed(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let (kept, mut changes) = (self.clone(), self.0.subscribe());
        async move {
            // Never an error: `kept` holds the sender while this waits.
            let _ = changes.changed().await;
            drop(kept);
        }
    }
}
