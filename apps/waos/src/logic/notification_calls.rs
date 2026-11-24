//! Where the programs' notifications go: each post, schedule and removal to the main thread, where
//! Waft's notifications are (`state::notifications`), with the program it is of. The engine calls
//! from a program's thread once the program may post, and is answered later, through the future it
//! was given. Whether the system takes replies and schedules is read once, as WaOS starts: a
//! system's notifications offer the same for as long as WaOS runs.
//!
//! The invariant: every post and schedule asked for reaches the main thread or is answered at once,
//! so no program waits for an answer that cannot come.

use std::future::ready;
use std::time::SystemTime;

use futures::FutureExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waos::bindings::wasdk::platform::notifications::NotificationError;
use waos::{
    Notification, NotificationFuture, RunningProgram, ShellNotificationScheduling,
    ShellNotifications,
};

/// One call of a program's, on its way to the main thread.
pub(crate) struct NotificationCall {
    /// The program the notification is of.
    pub(crate) program: RunningProgram,
    pub(crate) request: NotificationRequest,
}

/// What a program asks of the system's notifications.
pub(crate) enum NotificationRequest {
    /// Show `notification` now, or at `at`.
    Post {
        notification: Notification,
        at: Option<SystemTime>,
        /// Whether the system has it, or why not.
        posted: oneshot::Sender<Result<(), NotificationError>>,
    },
    /// Take its notification `id` away, shown or scheduled.
    Remove { id: String },
}

/// What the system's notifications offer beyond posting.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Offered {
    pub(crate) replies: bool,
    pub(crate) scheduling: bool,
}

/// The app's notifications, as the engine reaches them: every call goes to the main thread.
#[derive(Clone)]
pub(crate) struct NotificationsOnRequest {
    calls: UnboundedSender<NotificationCall>,
    offered: Offered,
}

impl NotificationsOnRequest {
    /// The notifications of a system offering `offered`, and where their calls arrive on the
    /// main thread.
    pub(crate) fn new(
        offered: Offered,
    ) -> (NotificationsOnRequest, UnboundedReceiver<NotificationCall>) {
        let (calls, arriving) = mpsc::unbounded();
        (NotificationsOnRequest { calls, offered }, arriving)
    }

    /// Sends `notification` of `program`'s to the main thread, to show now or at `at`, and answers
    /// whether the system has it.
    fn show(
        &self,
        program: &RunningProgram,
        notification: &Notification,
        at: Option<SystemTime>,
    ) -> NotificationFuture {
        let (posted, answered) = oneshot::channel();
        let request = NotificationRequest::Post {
            notification: notification.clone(),
            at,
            posted,
        };
        if !self.send(program, request) {
            return Box::pin(ready(Err(quitting())));
        }
        Box::pin(answered.map(|posted| posted.unwrap_or_else(|_| Err(quitting()))))
    }

    /// Sends `request` of `program`'s to the main thread; false once WaOS is quitting.
    fn send(&self, program: &RunningProgram, request: NotificationRequest) -> bool {
        let call = NotificationCall {
            program: program.clone(),
            request,
        };
        let sent = self.calls.unbounded_send(call).is_ok();
        if !sent {
            tracing::info!("WaOS is quitting: a notification is left as it is");
        }
        sent
    }
}

impl ShellNotifications for NotificationsOnRequest {
    fn post(&self, program: &RunningProgram, notification: &Notification) -> NotificationFuture {
        self.show(program, notification, None)
    }

    fn remove(&self, program: &RunningProgram, id: &str) {
        self.send(program, NotificationRequest::Remove { id: id.to_owned() });
    }

    fn offers_replies(&self) -> bool {
        self.offered.replies
    }

    fn scheduling(&self) -> Option<&dyn ShellNotificationScheduling> {
        self.offered
            .scheduling
            .then_some(self as &dyn ShellNotificationScheduling)
    }
}

impl ShellNotificationScheduling for NotificationsOnRequest {
    fn schedule(
        &self,
        program: &RunningProgram,
        notification: &Notification,
        at: SystemTime,
    ) -> NotificationFuture {
        self.show(program, notification, Some(at))
    }
}

fn quitting() -> NotificationError {
    NotificationError::Failed("WaOS is quitting".to_owned())
}
