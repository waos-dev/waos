//! What Permissions' tests stand in with: the package manager's removals, played by the test.

#![allow(dead_code, reason = "each test crate uses its own part of the support")]

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures::FutureExt;
use tokio::sync::{mpsc, watch};
use waos_capabilities::ChangedFuture;
use waos_package::ProgramId;
use waos_permissions::InstalledApps;

pub fn triangle() -> ProgramId {
    "dev.example.triangle".parse().unwrap()
}

/// The package manager's removals, as a test begins and ends them.
#[derive(Default)]
pub struct Removals {
    removing: watch::Sender<BTreeSet<ProgramId>>,
    listeners: Mutex<Vec<mpsc::UnboundedSender<ProgramId>>>,
}

impl Removals {
    /// The removal of `app` begins.
    pub fn begin(&self, app: &ProgramId) {
        self.removing.send_modify(|removing| {
            removing.insert(app.clone());
        });
    }

    /// `app` is out of the record: its removal is told, and ends.
    pub fn end(&self, app: &ProgramId) {
        for listener in self.listeners.lock().unwrap().iter() {
            let _ = listener.send(app.clone());
        }
        self.removing.send_modify(|removing| {
            removing.remove(app);
        });
    }
}

impl InstalledApps for Removals {
    fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.removing.subscribe()
    }

    fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId> {
        let (sender, removals) = mpsc::unbounded_channel();
        self.listeners.lock().unwrap().push(sender);
        removals
    }
}

/// Whether `changed` resolves within a moment.
pub fn heard(mut changed: ChangedFuture) -> bool {
    let deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < deadline {
        if changed.as_mut().now_or_never().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}
