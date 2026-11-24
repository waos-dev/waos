//! What macOS says of each program's notifications, as the program's app reads it and tells WaOS:
//! as WaOS welcomed the app, as macOS's question is answered, as the app or WaOS comes to the
//! front, and as a post is answered. Kept here for the engine, which asks each time it reads the
//! program's permission (`AppNotifications`, its `ShellSystemPermissions`), and each reading sent
//! on to the main thread, where WaOS's settings page for the program says so.
//!
//! The invariant: only what an app said is kept, never what WaOS's user answered in WaOS, and a
//! program whose app said nothing yet is not asked, so the engine has macOS asked for it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use waos::SystemPermissionState;
use waos_package::ProgramId;

/// A reading of macOS's permission for a program's notifications, as its app told it: that macOS
/// refuses them, or does not.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NotificationPermissionReading {
    pub(crate) program: ProgramId,
    pub(crate) refuses: bool,
}

/// What macOS last said of each program's notifications. Every clone is the same record.
#[derive(Clone)]
pub(crate) struct NotificationPermissions {
    said: Arc<Mutex<HashMap<ProgramId, SystemPermissionState>>>,
    /// Where each reading goes, on its way to the main thread.
    readings: UnboundedSender<NotificationPermissionReading>,
}

impl NotificationPermissions {
    /// The record, nothing said yet, and where each reading arrives for the main thread.
    pub(crate) fn new() -> (
        NotificationPermissions,
        UnboundedReceiver<NotificationPermissionReading>,
    ) {
        let (readings, arriving) = mpsc::unbounded();
        let permissions = NotificationPermissions {
            said: Arc::default(),
            readings,
        };
        (permissions, arriving)
    }

    /// What macOS last said of `program`'s notifications; not asked while its app said nothing.
    pub(crate) fn state(&self, program: &ProgramId) -> SystemPermissionState {
        let said = self.said.lock().unwrap_or_else(PoisonError::into_inner);
        (said.get(program).copied()).unwrap_or(SystemPermissionState::NotAsked)
    }

    /// `program`'s app read `state` of macOS: kept, and told to the main thread.
    pub(crate) fn said(&self, program: ProgramId, state: SystemPermissionState) {
        (self.said.lock().unwrap_or_else(PoisonError::into_inner)).insert(program.clone(), state);
        let reading = NotificationPermissionReading {
            program,
            refuses: state == SystemPermissionState::Denied,
        };
        let _ = self.readings.unbounded_send(reading);
    }
}
