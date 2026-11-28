//! The system's own permissions, as a system would keep them beside what a program's user allowed
//! it in WaOS: its permission for each program's notifications, which is the test notifications'
//! to say, and for the files and folders a test says it guards, which it denies until a test has
//! it allow them as it is asked, as a user turning the permission on in the system's settings.

use std::future::ready;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use waos::{
    AppInstance, ShellSystemPermissions, SystemPermission, SystemPermissionFuture,
    SystemPermissionState,
};
use waos_headless::testing::Changes;

use super::notifications::TestNotifications;

/// What the system itself allows of what the programs' users allowed them.
pub struct TestSystemPermissions {
    notifications: Arc<TestNotifications>,
    /// The folders the system denies WaOS, with everything in them.
    denied_folders: Mutex<Vec<PathBuf>>,
    /// Whether the system allows a path once it is asked for it.
    allows_when_asked: AtomicBool,
    /// Each path the system was asked for, in order.
    paths_asked: Mutex<Vec<PathBuf>>,
    changes: Arc<Changes>,
}

impl TestSystemPermissions {
    /// The system's permissions, its notifications' as `notifications` says, every path allowed,
    /// announcing each path asked for to `changes`.
    pub fn new(
        notifications: Arc<TestNotifications>,
        changes: &Arc<Changes>,
    ) -> TestSystemPermissions {
        TestSystemPermissions {
            notifications,
            denied_folders: Mutex::default(),
            allows_when_asked: AtomicBool::new(false),
            paths_asked: Mutex::default(),
            changes: Arc::clone(changes),
        }
    }

    /// The system denies WaOS `folder` and everything in it from now on.
    pub fn deny_folder(&self, folder: &Path) {
        self.denied_folders.lock().unwrap().push(folder.to_owned());
    }

    /// From now on the system allows a path as it is asked for it, as a user turning it on.
    pub fn allow_when_asked(&self) {
        self.allows_when_asked.store(true, Ordering::SeqCst);
    }

    /// Each path the system was asked for, in order.
    pub fn paths_asked(&self) -> Vec<PathBuf> {
        self.paths_asked.lock().unwrap().clone()
    }

    fn path_state(&self, path: &Path) -> SystemPermissionState {
        let denied = self.denied_folders.lock().unwrap();
        if denied.iter().any(|folder| path.starts_with(folder)) {
            SystemPermissionState::Denied
        } else {
            SystemPermissionState::Allowed
        }
    }

    /// The system asked for `path`: allowed with every folder it denied, where a test said so.
    fn request_path(&self, path: &Path) -> SystemPermissionState {
        self.paths_asked.lock().unwrap().push(path.to_owned());
        if self.allows_when_asked.load(Ordering::SeqCst) {
            self.denied_folders.lock().unwrap().clear();
        }
        self.changes.announce();
        self.path_state(path)
    }
}

impl ShellSystemPermissions for TestSystemPermissions {
    fn state(
        &self,
        permission: SystemPermission<'_>,
        program: &AppInstance,
    ) -> SystemPermissionState {
        match permission {
            SystemPermission::Notifications => self.notifications.permission_state(program),
            SystemPermission::Path(path) => self.path_state(path),
        }
    }

    fn request(
        &self,
        permission: SystemPermission<'_>,
        program: &AppInstance,
    ) -> SystemPermissionFuture {
        let state = match permission {
            SystemPermission::Notifications => self.notifications.request_permission(program),
            SystemPermission::Path(path) => self.request_path(path),
        };
        Box::pin(ready(state))
    }
}
