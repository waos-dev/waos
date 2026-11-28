//! The system's notifications, as the platform would keep them: each program's shown under its
//! name and its own id, a post of the same id replacing it, each removal and each schedule kept
//! as it was asked, every call answered at once; with replies and scheduling, unless a test takes
//! them away; refusing every program's notifications while a test says the system does; and,
//! once a test says so, keeping a permission of its own for each program, as macOS keeps one for
//! each program's app: not asked until its question is put or a post is answered, then denied or
//! allowed as the system refused or took it, which is what the macOS shell learns and says
//! (`system_permissions`).

use std::collections::HashMap;
use std::future::ready;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use waos::bindings::wasdk::platform::notifications::NotificationError;
use waos::{
    Notification, NotificationFuture, RunningProgram, ShellNotificationScheduling,
    ShellNotifications, SystemPermissionState,
};
use waos_headless::testing::Changes;

/// What the programs posted, removed and scheduled.
pub struct TestNotifications {
    /// What is shown now, by the program's name, in the order first posted.
    shown: Mutex<Vec<(String, Notification)>>,
    removed: Mutex<Vec<(String, String)>>,
    scheduled: Mutex<Vec<Scheduled>>,
    offers_replies: AtomicBool,
    offers_scheduling: AtomicBool,
    /// Whether the system refuses the programs' notifications.
    refusing: AtomicBool,
    /// Each post and schedule the system refused, by program and id, in order.
    refused: Mutex<Vec<(String, String)>>,
    /// Whether the system keeps a permission of its own for each program.
    keeps_permissions: AtomicBool,
    /// Each program the system's own question was put for, by name, in order.
    asked: Mutex<Vec<String>>,
    /// What the system last said of each program's permission, by the program's name.
    permissions: Mutex<HashMap<String, SystemPermissionState>>,
    changes: Arc<Changes>,
}

/// A notification a program scheduled.
#[derive(Clone, Debug)]
pub struct Scheduled {
    pub program: String,
    pub notification: Notification,
    pub at: SystemTime,
}

impl Default for TestNotifications {
    fn default() -> TestNotifications {
        TestNotifications::announcing(&Arc::default())
    }
}

impl TestNotifications {
    /// Nothing posted, with replies and scheduling, announcing each call to `changes`.
    pub fn announcing(changes: &Arc<Changes>) -> TestNotifications {
        TestNotifications {
            shown: Mutex::default(),
            removed: Mutex::default(),
            scheduled: Mutex::default(),
            offers_replies: AtomicBool::new(true),
            offers_scheduling: AtomicBool::new(true),
            refusing: AtomicBool::new(false),
            refused: Mutex::default(),
            keeps_permissions: AtomicBool::new(false),
            asked: Mutex::default(),
            permissions: Mutex::default(),
            changes: Arc::clone(changes),
        }
    }
}

impl TestNotifications {
    /// No replies and no scheduling from now on, as a system without them.
    pub fn without_replies_or_scheduling(&self) {
        self.offers_replies.store(false, Ordering::SeqCst);
        self.offers_scheduling.store(false, Ordering::SeqCst);
    }

    /// The system refuses every post and schedule from now on, answering `denied` and showing
    /// nothing, or, with `false`, takes them again.
    pub fn refuse(&self, refusing: bool) {
        self.refusing.store(refusing, Ordering::SeqCst);
    }

    /// Each post and schedule the system refused, by program and id, in order.
    pub fn refused(&self) -> Vec<(String, String)> {
        self.refused.lock().unwrap().clone()
    }

    /// The system keeps a permission of its own for each program from now on.
    pub fn keep_permissions(&self) {
        self.keeps_permissions.store(true, Ordering::SeqCst);
    }

    /// Each program the system's own question was put for, by name, in order.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    /// Whether the system refuses `program`'s `notification`, which it keeps as refused; a system
    /// that keeps a permission for each program has said what it did of the program's.
    fn refuses(&self, program: &RunningProgram, notification: &Notification) -> bool {
        let refusing = self.refusing.load(Ordering::SeqCst);
        if refusing {
            let refused = (program.name().to_owned(), notification.id.clone());
            self.refused.lock().unwrap().push(refused);
        }
        self.read_permission(program);
        self.changes.announce();
        refusing
    }

    /// What the system last said of its permission for `program`'s notifications: allowed where it
    /// keeps none for a program, not asked while it said nothing.
    pub fn permission_state(&self, program: &RunningProgram) -> SystemPermissionState {
        if !self.keeps_permissions.load(Ordering::SeqCst) {
            return SystemPermissionState::Allowed;
        }
        let said = self.permissions.lock().unwrap();
        (said.get(program.name()).copied()).unwrap_or(SystemPermissionState::NotAsked)
    }

    /// The system's question for `program`'s notifications, answered at once.
    pub fn request_permission(&self, program: &RunningProgram) -> SystemPermissionState {
        self.asked.lock().unwrap().push(program.name().to_owned());
        let state = self.read_permission(program);
        self.changes.announce();
        state
    }

    /// The system's permission for `program` as it stands now, which it has said from now on.
    fn read_permission(&self, program: &RunningProgram) -> SystemPermissionState {
        let state = if self.refusing.load(Ordering::SeqCst) {
            SystemPermissionState::Denied
        } else {
            SystemPermissionState::Allowed
        };
        let name = program.name().to_owned();
        self.permissions.lock().unwrap().insert(name, state);
        state
    }

    /// What is shown now, with the name of the program each is of.
    pub fn shown(&self) -> Vec<(String, Notification)> {
        self.shown.lock().unwrap().clone()
    }

    /// Each removal asked for, by program and id, in order.
    pub fn removed(&self) -> Vec<(String, String)> {
        self.removed.lock().unwrap().clone()
    }

    /// Each notification scheduled, in order.
    pub fn scheduled(&self) -> Vec<Scheduled> {
        self.scheduled.lock().unwrap().clone()
    }
}

impl ShellNotifications for TestNotifications {
    fn post(&self, program: &RunningProgram, notification: &Notification) -> NotificationFuture {
        if self.refuses(program, notification) {
            return Box::pin(ready(Err(NotificationError::Denied)));
        }
        let mut shown = self.shown.lock().unwrap();
        let name = program.name();
        let same = shown
            .iter_mut()
            .find(|(of, kept)| of == name && kept.id == notification.id);
        match same {
            Some((_, kept)) => *kept = notification.clone(),
            None => shown.push((name.to_owned(), notification.clone())),
        }
        drop(shown);
        self.changes.announce();
        Box::pin(ready(Ok(())))
    }

    fn remove(&self, program: &RunningProgram, id: &str) {
        let name = program.name();
        self.removed
            .lock()
            .unwrap()
            .push((name.to_owned(), id.to_owned()));
        self.shown
            .lock()
            .unwrap()
            .retain(|(of, kept)| !(of == name && kept.id == id));
        self.changes.announce();
    }

    fn offers_replies(&self) -> bool {
        self.offers_replies.load(Ordering::SeqCst)
    }

    fn scheduling(&self) -> Option<&dyn ShellNotificationScheduling> {
        let offers = self.offers_scheduling.load(Ordering::SeqCst);
        offers.then_some(self as &dyn ShellNotificationScheduling)
    }
}

impl ShellNotificationScheduling for TestNotifications {
    fn schedule(
        &self,
        program: &RunningProgram,
        notification: &Notification,
        at: SystemTime,
    ) -> NotificationFuture {
        if self.refuses(program, notification) {
            return Box::pin(ready(Err(NotificationError::Denied)));
        }
        self.scheduled.lock().unwrap().push(Scheduled {
            program: program.name().to_owned(),
            notification: notification.clone(),
            at,
        });
        self.changes.announce();
        Box::pin(ready(Ok(())))
    }
}
