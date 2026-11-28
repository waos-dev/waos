//! The installs in flight, each a session, as Android's `PackageInstaller.Session` is: from the
//! moment it is asked for, with its app, who asked, how far it is, and a way to cancel it. One
//! list of them is told to whoever shows them.
//!
//! The invariants: a session is in the list from the call that asked for it until its work has
//! ended, whatever ended it, its files dropped; a caller's newer session for an app cancels the
//! caller's older one for it as the newer begins, and touches no other caller's.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::{oneshot, watch};
use waos_package::ProgramId;

use crate::packages::error::InstallError;
use crate::packages::installed::InstalledProgram;

/// Who asked for an install, which decides what it may do. Today WaOS's own parts alone ask, as
/// the user's own hands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Caller {
    /// The user, through WaOS's own UI: its library, and a package opened with WaOS.
    User,
    /// The developer tools: `waos install`, through the dev server.
    DeveloperTools,
}

/// How far an install is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallProgress {
    /// Its package is being received: the bytes taken in so far, of the total its caller said,
    /// where it said one.
    Receiving { received: u64, total: Option<u64> },
    /// Its package is in and being checked, unpacked and compiled; it is installed once that is
    /// done.
    Installing,
}

/// One session in flight, as whoever shows them sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallSessionInfo {
    /// Tells it apart from the other sessions, of its app and of the others.
    pub session_id: u64,
    pub app: ProgramId,
    pub caller: Caller,
    pub progress: InstallProgress,
}

/// An install in flight, as its caller holds it, from the moment it is asked for: its package's
/// bytes may start later, as a download's do once its server answers. Dropping it leaves the
/// install going; cancel it to stop it.
pub struct InstallSession {
    pub(crate) app: ProgramId,
    pub(crate) caller: Caller,
    pub(crate) progress: watch::Receiver<InstallProgress>,
    pub(crate) size: Arc<watch::Sender<Option<u64>>>,
    pub(crate) cancel: Arc<watch::Sender<bool>>,
    pub(crate) answer: Mutex<Option<oneshot::Receiver<Result<InstalledProgram, InstallError>>>>,
}

impl InstallSession {
    /// The app it installs.
    pub fn app(&self) -> &ProgramId {
        &self.app
    }

    /// Who asked for it.
    pub fn caller(&self) -> Caller {
        self.caller
    }

    /// How far it is now, and each change from now on.
    pub fn progress(&self) -> watch::Receiver<InstallProgress> {
        self.progress.clone()
    }

    /// Its package is `size` bytes long, as whoever feeds it learns once its bytes begin, as
    /// Android's session is told the length as its writing opens: its progress has a total from
    /// now on.
    pub fn set_size(&self, size: u64) {
        self.size.send_replace(Some(size));
    }

    /// Stops it, unless it is installing its app already: what it received goes, and the app
    /// stays as it was.
    pub fn cancel(&self) {
        self.cancel.send_replace(true);
    }

    /// What it came to: the app installed, or why not. Answered once; a second wait answers
    /// `Cancelled`.
    pub async fn finished(&self) -> Result<InstalledProgram, InstallError> {
        let answer = self
            .answer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match answer {
            Some(answer) => answer.await.unwrap_or(Err(InstallError::Cancelled)),
            None => Err(InstallError::Cancelled),
        }
    }
}

/// The sessions in flight.
#[derive(Default)]
pub(crate) struct Sessions {
    list: Mutex<Vec<Entry>>,
    published: watch::Sender<Vec<InstallSessionInfo>>,
    serials: AtomicU64,
}

/// One session in the list, and what cancels it.
struct Entry {
    info: InstallSessionInfo,
    cancel: Arc<watch::Sender<bool>>,
}

/// A session begun: its id, what cancels it, and what tells its work it was cancelled.
pub(crate) struct Begun {
    pub(crate) session_id: u64,
    pub(crate) cancel: Arc<watch::Sender<bool>>,
    pub(crate) cancelled: watch::Receiver<bool>,
}

impl Sessions {
    /// Begins a session of `app` for `caller`, at `first`, cancelling the caller's older session
    /// for the app, if any.
    pub(crate) fn begin(&self, app: &ProgramId, caller: Caller, first: InstallProgress) -> Begun {
        let (cancel, cancelled) = watch::channel(false);
        let cancel = Arc::new(cancel);
        let session_id = self.serials.fetch_add(1, Ordering::SeqCst);
        let mut list = self.listed();
        for older in list
            .iter()
            .filter(|entry| entry.info.app == *app && entry.info.caller == caller)
        {
            tracing::info!(%app, ?caller, "a newer session cancels the older one");
            older.cancel.send_replace(true);
        }
        list.push(Entry {
            info: InstallSessionInfo {
                session_id,
                app: app.clone(),
                caller,
                progress: first,
            },
            cancel: Arc::clone(&cancel),
        });
        self.publish(&list);
        Begun {
            session_id,
            cancel,
            cancelled,
        }
    }

    /// Cancels every session of `app`; answers their ids.
    pub(crate) fn cancel_all_of(&self, app: &ProgramId) -> Vec<u64> {
        self.listed()
            .iter()
            .filter(|entry| entry.info.app == *app)
            .map(|entry| {
                entry.cancel.send_replace(true);
                entry.info.session_id
            })
            .collect()
    }

    /// Waits until none of the sessions `ids` is in flight.
    pub(crate) async fn ended(&self, ids: &[u64]) {
        let mut published = self.published.subscribe();
        // The sender lives as long as `self`, so the wait ends only when they have.
        let _ = published
            .wait_for(|list| list.iter().all(|info| !ids.contains(&info.session_id)))
            .await;
    }

    /// The session `session_id` is at `progress` now.
    pub(crate) fn set(&self, session_id: u64, progress: InstallProgress) {
        let mut list = self.listed();
        if let Some(entry) = list
            .iter_mut()
            .find(|entry| entry.info.session_id == session_id)
        {
            entry.info.progress = progress;
        }
        self.publish(&list);
    }

    /// The session `session_id` has ended.
    pub(crate) fn end(&self, session_id: u64) {
        let mut list = self.listed();
        list.retain(|entry| entry.info.session_id != session_id);
        self.publish(&list);
    }

    /// The sessions in flight now, and each change from now on.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Vec<InstallSessionInfo>> {
        self.published.subscribe()
    }

    /// Tells the watchers `list`, under its lock, so they hear the changes in their order.
    fn publish(&self, list: &[Entry]) {
        self.published
            .send_replace(list.iter().map(|entry| entry.info.clone()).collect());
    }

    /// The list, for a moment: never held across an await.
    fn listed(&self) -> MutexGuard<'_, Vec<Entry>> {
        // Every change leaves the list whole, so one that panicked leaves nothing half-changed.
        self.list.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
