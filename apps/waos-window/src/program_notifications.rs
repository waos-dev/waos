//! The program's notifications in its app (decision 14 of `docs/proposals/2026-10-29-macos-apps.md`):
//! posted, scheduled and removed through the window process's Waft host, under the app's bundle,
//! so they show under the program's name and icon, and macOS holds a scheduled one under that
//! bundle whether or not the app runs. Each keeps the program's own id and title, the bundle being
//! the program's alone. macOS's own question for the app is put when WaOS asks for it, which the
//! engine does after WaOS's question, and the first time WaOS asks for a post or a schedule, for a
//! program allowed in WaOS's settings that never asked; a post macOS refuses answers `denied`, as
//! the host's does. Whether macOS refuses the program its notifications goes to WaOS as macOS
//! answers its question, as WaOS welcomes the app, each time the app comes to the front
//! (`PermissionAtTheFront`), and each time WaOS asks, as WaOS comes to the front, the user having
//! perhaps changed it in System Settings meanwhile, as Chromium's notification service reads it
//! again each time its app becomes active
//! (`MacNotificationServiceUN::OnApplicationDidBecomeActive`); a user sent there by WaOS's
//! settings page comes back to WaOS, not to the app. What the user does to one of the
//! program's notifications goes to WaOS as the program hears it (`NotificationClicks`): once WaOS
//! welcomed the app, as it comes; before, it waits, and the first is the reason the app was
//! launched when the user's click launched it, which the hello carries.
//!
//! The invariant a later editor will break: every post, schedule and question WaOS asks for is
//! answered once, in the order asked, with why where the host has no notifications, and none is
//! posted before macOS's question was put; and nothing waits on WaOS.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::ready;
use std::rc::Rc;
use std::time::SystemTime;

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedSender};
use waft::ui::{
    AppLifecycleState, NotificationError as WaftError, NotificationPermission, PermissionState,
    PlatformRef, PostFuture,
};
use waft::{App, AsyncApp, WidgetsBinding, WidgetsBindingObserver};
use waos::Notification;
use waos::bindings::wasdk::platform::notifications::{Activation, NotificationError};
use waos_program_window::{activation_of, waft_notification};
use waos_window_messages::{FromWindow, LaunchReason, PermissionRequestId, PostId};

use crate::connection::Connection;
use crate::finding_waos::Launcher;

/// A post or a schedule WaOS asks the window process for.
pub(crate) struct AskedNotification {
    /// What its answer names.
    pub(crate) post: PostId,
    pub(crate) notification: Notification,
    /// When the system shows it; none for now.
    pub(crate) at: Option<SystemTime>,
}

/// The program's notifications in its app, carried out in the order WaOS asks. Every clone is the
/// same.
#[derive(Clone)]
pub(crate) struct ProgramNotifications {
    asked: UnboundedSender<Asked>,
}

/// What is carried out for the program's notifications: what WaOS asks, and the app's own reads of
/// macOS's permission beside WaOS's.
enum Asked {
    Post(AskedNotification),
    /// Take the notification of this id away, shown or scheduled.
    Removal(String),
    /// Put macOS's question for the app, answering WaOS under this id.
    PermissionQuestion(PermissionRequestId),
    /// Read macOS's permission for the app, and tell WaOS.
    PermissionReading,
}

impl ProgramNotifications {
    /// From now on, what WaOS asks of the program's notifications is carried out through the
    /// host's, a post and macOS's question answered to WaOS through `connection`; and whether
    /// macOS refuses the program its notifications is told to WaOS now and each time the app comes
    /// to the front.
    pub(crate) fn serve(app: &mut App, connection: Rc<Connection>) -> ProgramNotifications {
        let (asked, mut arriving) = mpsc::unbounded();
        let mut serving = Serving {
            platform: app.platform(),
            connection,
            permission_asked: false,
        };
        app.spawn(async move |cx| {
            while let Some(asked) = arriving.next().await {
                serving.carry_out(asked, cx).await;
            }
        });
        let notifications = ProgramNotifications { asked };
        notifications.tell_permission();
        let at_the_front = Rc::new(PermissionAtTheFront(notifications.clone()));
        WidgetsBinding::instance(app).add_observer(app, at_the_front);
        notifications
    }

    /// Puts macOS's question for the app, after what was asked before it, and answers WaOS with
    /// `request` once macOS has answered.
    pub(crate) fn request_permission(&self, request: PermissionRequestId) {
        let _ = self
            .asked
            .unbounded_send(Asked::PermissionQuestion(request));
    }

    /// Reads macOS's permission for the app, after what was asked before, and tells WaOS.
    pub(crate) fn tell_permission(&self) {
        let _ = self.asked.unbounded_send(Asked::PermissionReading);
    }

    /// Posts or schedules `asked`, after what was asked before it.
    pub(crate) fn post(&self, asked: AskedNotification) {
        let _ = self.asked.unbounded_send(Asked::Post(asked));
    }

    /// Takes the program's notification `id` away, shown or scheduled, after what was asked
    /// before it.
    pub(crate) fn remove(&self, id: String) {
        let _ = self.asked.unbounded_send(Asked::Removal(id));
    }
}

/// What carries out WaOS's asks, in the order they come.
struct Serving {
    platform: PlatformRef,
    /// Where a post is answered.
    connection: Rc<Connection>,
    /// Whether macOS's question for the app was put.
    permission_asked: bool,
}

/// What tells WaOS macOS's permission for the app each time the app comes to the front, as Waft's
/// host says it is resumed.
struct PermissionAtTheFront(ProgramNotifications);

impl WidgetsBindingObserver for PermissionAtTheFront {
    fn did_change_app_lifecycle_state(&self, _app: &mut App, state: AppLifecycleState) {
        if state == AppLifecycleState::Resumed {
            self.0.tell_permission();
        }
    }
}

impl Serving {
    /// Carries out `asked`; a post is answered once the system has it, while later asks go on.
    async fn carry_out(&mut self, asked: Asked, cx: &mut AsyncApp) {
        match asked {
            Asked::Post(asked) => {
                self.ask_permission_once().await;
                let posted = self.post(&asked.notification, asked.at);
                let (connection, post) = (Rc::clone(&self.connection), asked.post);
                cx.update(|app| {
                    app.spawn(async move |_cx| {
                        let posted = posted.await.map_err(program_error);
                        connection.send(FromWindow::NotificationPosted { post, posted });
                    })
                });
            }
            Asked::Removal(id) => {
                if let Some(notifications) = self.platform.notifications() {
                    notifications.remove(&id);
                }
            }
            Asked::PermissionQuestion(request) => {
                let refused = self.ask_permission().await;
                let answer = FromWindow::NotificationPermissionAnswered { request, refused };
                self.connection.send(answer);
            }
            Asked::PermissionReading => {
                let refused = self.read_permission().await;
                self.connection
                    .send(FromWindow::NotificationPermission { refused });
            }
        }
    }

    /// Puts macOS's question for the app before its first post, unless it was put before, and
    /// tells WaOS what macOS answered.
    async fn ask_permission_once(&mut self) {
        if self.permission_asked {
            return;
        }
        let refused = self.ask_permission().await;
        self.connection
            .send(FromWindow::NotificationPermission { refused });
    }

    /// Puts macOS's question for the app, where the system keeps a permission for it, which asks
    /// the user only if they were never asked; answers whether macOS refuses the program its
    /// notifications, never where the system keeps none.
    async fn ask_permission(&mut self) -> bool {
        self.permission_asked = true;
        let Some(asked) = self.permission().map(NotificationPermission::request) else {
            return false;
        };
        let state = asked.await;
        tracing::info!(?state, "macOS's permission for the program's notifications");
        refuses(state)
    }

    /// Whether macOS refuses the program its notifications now, read without asking; never where
    /// the system keeps no permission for the app.
    async fn read_permission(&self) -> bool {
        match self.permission().map(NotificationPermission::state) {
            Some(read) => refuses(read.await),
            None => false,
        }
    }

    /// The system's permission for the app, where it keeps one.
    fn permission(&self) -> Option<&dyn NotificationPermission> {
        self.platform.notifications()?.permission()
    }

    /// Posts `notification` through the host's notifications, now or at `at`.
    fn post(&self, notification: &Notification, at: Option<SystemTime>) -> PostFuture {
        let Some(notifications) = self.platform.notifications() else {
            return failed("Waft's platform has no notifications to post");
        };
        let shown = waft_notification(notification, notifications.replies());
        match (at, notifications.scheduling()) {
            (None, _) => notifications.post(&shown),
            (Some(at), Some(scheduling)) => scheduling.schedule(&shown, at),
            (Some(_), None) => failed("Waft's platform schedules no notification"),
        }
    }
}

/// Whether the permission `state` refuses the program its notifications: only where the user, or
/// System Settings, said no; one never asked is not refused, its question coming before a post.
fn refuses(state: PermissionState) -> bool {
    state == PermissionState::Denied
}

fn failed(why: &str) -> PostFuture {
    Box::pin(ready(Err(WaftError::Failed(why.to_owned()))))
}

/// Why the system did not take a notification, as the program hears it: macOS refusing the app is
/// `denied`.
fn program_error(error: WaftError) -> NotificationError {
    match error {
        WaftError::Denied => NotificationError::Denied,
        WaftError::Failed(reason) => NotificationError::Failed(reason),
    }
}

/// What the user does to the program's notifications: collected until WaOS welcomes the app, and
/// sent to WaOS as it comes from then on. Every clone is the same clicks.
#[derive(Clone)]
pub(crate) struct NotificationClicks(Rc<RefCell<Clicks>>);

enum Clicks {
    Collecting(VecDeque<Activation>),
    Connected(Rc<Connection>),
}

impl NotificationClicks {
    /// From now on, each click Waft's host reports is collected, or sent once WaOS welcomed the
    /// app; the click that launched the app is among them, reported once the app runs.
    pub(crate) fn listen(app: &mut App) -> NotificationClicks {
        let clicks = NotificationClicks(Rc::new(RefCell::new(Clicks::Collecting(VecDeque::new()))));
        let heard = clicks.clone();
        app.set_notification_activated_handler(Some(Rc::new(move |_app, activation| {
            heard.clicked(activation_of(activation));
        })));
        clicks
    }

    /// Why the app was launched, by `launcher`, as the hello says it: the first click collected,
    /// taken, when the user's click launched it.
    pub(crate) fn launch_reason(&self, launcher: Launcher) -> LaunchReason {
        if launcher == Launcher::Waos {
            return LaunchReason::ProgramRunning;
        }
        let first = match &mut *self.0.borrow_mut() {
            Clicks::Collecting(collected) => collected.pop_front(),
            Clicks::Connected(_) => None,
        };
        match first {
            Some(click) => LaunchReason::NotificationActivated(click),
            None => LaunchReason::UserOpened,
        }
    }

    /// WaOS welcomed the app: the clicks still collected go to it through `connection`, in order,
    /// and every later one as it comes.
    pub(crate) fn connected(&self, connection: Rc<Connection>) {
        let before = std::mem::replace(
            &mut *self.0.borrow_mut(),
            Clicks::Connected(Rc::clone(&connection)),
        );
        if let Clicks::Collecting(collected) = before {
            for click in collected {
                connection.send(FromWindow::NotificationActivated(click));
            }
        }
    }

    fn clicked(&self, click: Activation) {
        match &mut *self.0.borrow_mut() {
            Clicks::Collecting(collected) => collected.push_back(click),
            Clicks::Connected(connection) => {
                connection.send(FromWindow::NotificationActivated(click));
            }
        }
    }
}

#[cfg(test)]
mod tests;
