//! The system's notifications, as the platform would keep them: each program's shown under its
//! name and its own id, a post of the same id replacing it, each removal and each schedule kept
//! as it was asked, every call answered at once; with replies and scheduling, unless a test takes
//! them away; refusing every program's notifications while a test says the system does; and,
//! once a test says so, keeping a permission of its own for each program, as macOS keeps one for
//! each program's app: its question answers whether it refuses them, and each post and schedule
//! it refuses or takes is reported to the program, as the macOS shell reports it.

use std::future::ready;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use waos::bindings::wasdk::platform::notifications::NotificationError;
use waos::{
    Notification, NotificationFuture, RunningProgram, ShellNotificationScheduling,
    ShellNotifications, ShellSystemPermission, SystemPermissionFuture,
};

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
    /// that keeps a permission for each program reports what it did to the program.
    fn refuses(&self, program: &RunningProgram, notification: &Notification) -> bool {
        let refusing = self.refusing.load(Ordering::SeqCst);
        if refusing {
            let refused = (program.name().to_owned(), notification.id.clone());
            self.refused.lock().unwrap().push(refused);
        }
        if self.keeps_permissions.load(Ordering::SeqCst) {
            program.system_notifications_refused(refusing);
        }
        refusing
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
    }

    fn offers_replies(&self) -> bool {
        self.offers_replies.load(Ordering::SeqCst)
    }

    fn scheduling(&self) -> Option<&dyn ShellNotificationScheduling> {
        let offers = self.offers_scheduling.load(Ordering::SeqCst);
        offers.then_some(self as &dyn ShellNotificationScheduling)
    }

    fn system_permission(&self) -> Option<&dyn ShellSystemPermission> {
        let keeps = self.keeps_permissions.load(Ordering::SeqCst);
        keeps.then_some(self as &dyn ShellSystemPermission)
    }
}

/// The system's question, answered at once: it refuses the program while a test says it does.
impl ShellSystemPermission for TestNotifications {
    fn request(&self, program: &RunningProgram) -> SystemPermissionFuture {
        self.asked.lock().unwrap().push(program.name().to_owned());
        Box::pin(ready(Some(self.refusing.load(Ordering::SeqCst))))
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
        Box::pin(ready(Ok(())))
    }
}
