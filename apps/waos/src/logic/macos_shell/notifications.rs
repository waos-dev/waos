//! A program's notifications on macOS, its app's: the engine's notifications, each post, schedule
//! and removal taking the getter, so a program whose app does not run has it launched, with no
//! window and in the background, and the notification goes there. The window process posts it under
//! the program's bundle, so it shows under the program's name and icon, and macOS holds a scheduled
//! one under that bundle whether or not the app runs. macOS's permission for the app is the
//! engine's system permission for notifications here (`ShellSystemPermissions`): the engine reads
//! what the app last said of it, and asks for it after WaOS's question, once its user allowed the
//! program notifications in WaOS and macOS does not allow them yet, and the app puts macOS's
//! question, answering what macOS says then; the app also puts it before its first post, for a
//! program allowed in WaOS's settings that never asks. A post macOS refuses, at that question or
//! later in System Settings, answers `denied`. What the app says macOS said of the program's
//! notifications, as WaOS welcomed it, as the question is answered, as it comes to the front, as
//! WaOS asks and as a post is answered, is kept (`notification_permissions`) and told to WaOS's
//! settings page for the program, which says so and sends the user to System Settings, since macOS
//! asks only once. WaOS asks each app that runs to read it again as WaOS comes to the front
//! (`state::program_apps`), since a user the page sent to System Settings comes back to WaOS, not
//! to the program's app; an app that does not run is not launched for it. What the user does to one
//! comes back from the window process to the program, and a click that launched the app comes with
//! its hello (`server`).
//!
//! macOS's notifications take replies and schedule (`UNTextInputNotificationAction`, calendar
//! triggers), and Waft's macOS host offers both wherever it has notifications, which a program's
//! app, always run from its bundle, has; so both are offered before any app runs, and so is the
//! system's permission.
//!
//! The invariant a later editor will break: every post, schedule and question asked for is
//! answered exactly once, by the app's answer, or with why the app never came or went first (a
//! question answering nothing then), so no program waits on a notification nobody posts; and
//! nothing here brings the app forward.

use std::future::ready;
use std::sync::Arc;
use std::time::SystemTime;

use waos::bindings::wasdk::platform::notifications::NotificationError;
use waos::{
    Notification, NotificationFuture, RunningProgram, ShellNotificationScheduling,
    ShellNotifications, ShellSystemPermissions, SystemPermission, SystemPermissionFuture,
    SystemPermissionState,
};

use super::window_processes::{WindowProcesses, app_id};
use crate::logic::notification_permissions::NotificationPermissions;

/// The engine's notifications on macOS: a program's posted, scheduled and removed in its app, and
/// macOS's question for them put there, the app launched for them in the background when it does
/// not run.
pub(crate) struct AppNotifications {
    window_processes: Arc<WindowProcesses>,
    /// What each program's app said macOS said of its notifications.
    permissions: NotificationPermissions,
}

impl AppNotifications {
    /// The programs' notifications in their apps, as `window_processes` reach them, macOS's
    /// permission for each read from `permissions`.
    pub(crate) fn new(
        window_processes: Arc<WindowProcesses>,
        permissions: NotificationPermissions,
    ) -> AppNotifications {
        AppNotifications {
            window_processes,
            permissions,
        }
    }

    /// Puts macOS's question for `program`'s notifications in its app, launched for it if it does
    /// not run, and answers what macOS says then; what it said before where the app never came.
    fn request_from_macos(&self, program: &RunningProgram) -> SystemPermissionFuture {
        let app = self.window_processes.of(program);
        let before = self.permission_state(program);
        Box::pin(async move {
            let answered = match app.await {
                Ok(app) => app.request_notification_permission().await,
                Err(why) => {
                    tracing::warn!(why, "macOS was not asked for the program's notifications");
                    None
                }
            };
            answered.unwrap_or(before)
        })
    }

    /// What the app of `program` last said macOS says of its notifications; not asked for a
    /// program with no app, whose question then finds no app to put it.
    fn permission_state(&self, program: &RunningProgram) -> SystemPermissionState {
        match app_id(program) {
            Ok(id) => self.permissions.state(&id),
            Err(_) => SystemPermissionState::NotAsked,
        }
    }

    /// Has the app of each of `programs` that runs read macOS's permission for the program's
    /// notifications again, telling it as it tells every reading; launches no app for it, since an
    /// app reads it as WaOS welcomes it.
    pub(crate) fn read_system_permissions(&self, programs: &[RunningProgram]) {
        let running = programs
            .iter()
            .filter_map(|program| self.window_processes.running(program));
        for app in running {
            app.read_notification_permission();
        }
    }

    /// Posts `program`'s `notification` in its app, now or at `at`; answers once the system has
    /// it, or why not.
    fn post_in_app(
        &self,
        program: &RunningProgram,
        notification: &Notification,
        at: Option<SystemTime>,
    ) -> NotificationFuture {
        let app = self.window_processes.of(program);
        let notification = notification.clone();
        Box::pin(async move {
            match app.await {
                Ok(app) => app.post_notification(notification, at).await,
                Err(why) => Err(NotificationError::Failed(why)),
            }
        })
    }
}

impl ShellNotifications for AppNotifications {
    fn post(&self, program: &RunningProgram, notification: &Notification) -> NotificationFuture {
        self.post_in_app(program, notification, None)
    }

    fn remove(&self, program: &RunningProgram, id: &str) {
        let id = id.to_owned();
        self.window_processes
            .with_app(program, move |app| match app {
                Ok(app) => app.remove_notification(id),
                Err(why) => tracing::warn!(why, "a notification is left as it is"),
            });
    }

    fn offers_replies(&self) -> bool {
        true
    }

    fn scheduling(&self) -> Option<&dyn ShellNotificationScheduling> {
        Some(self)
    }
}

impl ShellNotificationScheduling for AppNotifications {
    fn schedule(
        &self,
        program: &RunningProgram,
        notification: &Notification,
        at: SystemTime,
    ) -> NotificationFuture {
        self.post_in_app(program, notification, Some(at))
    }
}

/// macOS's permission for each program's app: read from what the app last said, and its question
/// put in the program's app. A file or folder the user picked is allowed: macOS gives an app what
/// its user picks in its dialog.
impl ShellSystemPermissions for AppNotifications {
    fn state(
        &self,
        permission: SystemPermission<'_>,
        program: &RunningProgram,
    ) -> SystemPermissionState {
        match permission {
            SystemPermission::Notifications => self.permission_state(program),
            SystemPermission::Path(_) => SystemPermissionState::Allowed,
        }
    }

    fn request(
        &self,
        permission: SystemPermission<'_>,
        program: &RunningProgram,
    ) -> SystemPermissionFuture {
        match permission {
            SystemPermission::Notifications => self.request_from_macos(program),
            SystemPermission::Path(_) => Box::pin(ready(SystemPermissionState::Allowed)),
        }
    }
}

#[cfg(test)]
mod tests;
