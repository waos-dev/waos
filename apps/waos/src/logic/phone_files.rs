//! The phone's files: whether WaOS may reach the phone's shared storage by its paths, which on
//! Android is the user's to allow, as "All files access" (`MANAGE_EXTERNAL_STORAGE`), on a page of
//! Android's settings; and so This computer's home, that storage once WaOS may reach it, WaOS's own
//! folder before. It is the system's own permission beside what a program's user allowed it in
//! WaOS: as the user picks This computer for a program, or a file or folder on the phone's storage,
//! WaOS opens the system's page for it (`PhoneFilesOnRequest`, one request for both), and tells
//! them where they come back with it still off; its settings say whether it is on. Only Android has
//! it; elsewhere WaOS reaches what its user does.
//!
//! The invariant: the system is asked only on the main thread (`state::phone_files`), as WaOS
//! starts and each time it comes back to the front, which is enough, since the user changes the
//! access only in the system's settings, away from WaOS; what another thread needs of it is kept
//! where any thread reads it: This computer's home in a [`ThisComputerHome`], and whether WaOS may
//! reach the phone's storage in a [`SharedStorage`].

use std::future::{Future, ready};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waos::{
    AppInstance, ShellSystemPermissions, SystemPermission, SystemPermissionFuture,
    SystemPermissionState,
};
use waos_local::THIS_COMPUTER;
use waos_program_window::is_on_phone_storage;

/// What the system says of the phone's files, asked on the main thread alone: Android refuses a
/// call from a thread running on a stack other than its own, as a program's thread does while
/// wasmtime runs it.
pub(crate) trait PhoneFilesAccess {
    /// Whether WaOS may reach the phone's shared storage now.
    fn reachable(&self) -> bool;

    /// Where the phone's shared storage is, whether or not WaOS may reach it.
    fn storage(&self) -> PathBuf;

    /// The user's home on This computer: the shared storage while WaOS may reach it, WaOS's own
    /// folder before.
    fn home(&self) -> PathBuf;

    /// Opens the system's page where the user lets WaOS reach the phone's files; why not, when it
    /// does not open.
    fn open_switch(&self) -> Result<(), String>;
}

/// The folder This computer's commands start in when they name none, as the main thread last
/// read it; every clone is the same.
#[derive(Clone)]
pub(crate) struct ThisComputerHome(Arc<RwLock<PathBuf>>);

impl ThisComputerHome {
    pub(crate) fn new(home: PathBuf) -> ThisComputerHome {
        ThisComputerHome(Arc::new(RwLock::new(home)))
    }

    /// The home, from any thread.
    pub(crate) fn get(&self) -> PathBuf {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The home is `home` from now on.
    pub(crate) fn set(&self, home: PathBuf) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = home;
    }
}

/// The phone's shared storage, as the main thread last read it: where it is, and whether WaOS may
/// reach it, for any thread to read. Every clone is the same.
#[derive(Clone)]
pub(crate) struct SharedStorage {
    root: Arc<Path>,
    reachable: Arc<AtomicBool>,
}

impl SharedStorage {
    /// The shared storage at `root`, out of WaOS's reach until it is read.
    pub(crate) fn at(root: PathBuf) -> SharedStorage {
        SharedStorage {
            root: Arc::from(root),
            reachable: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Whether WaOS may reach it, from any thread.
    pub(crate) fn reachable(&self) -> bool {
        self.reachable.load(Ordering::SeqCst)
    }

    /// WaOS may reach it, or not, from now on.
    pub(crate) fn set_reachable(&self, reachable: bool) {
        self.reachable.store(reachable, Ordering::SeqCst);
    }

    /// Whether the system keeps WaOS from `path`: it is on the phone's own storage, which WaOS
    /// may not reach.
    pub(crate) fn keeps_from(&self, path: &Path) -> bool {
        !self.reachable() && is_on_phone_storage(path, &self.root)
    }
}

/// One request to let WaOS reach the phone's files, for something `program`'s user gave it, on its
/// way to the main thread.
pub(crate) struct PhoneFilesRequest {
    pub(crate) program: AppInstance,
    /// Where whether WaOS may reach them goes, once the user is back from the system's page.
    pub(crate) reachable: oneshot::Sender<bool>,
}

/// What asks the system to let WaOS reach the phone's files, from any thread: each request goes to
/// the main thread, which opens the system's page for them and answers once the user is back
/// (`state::phone_files`). It is the engine's system permissions on Android, whose All files
/// access guards what a user picks on the phone's storage.
#[derive(Clone)]
pub(crate) struct PhoneFilesOnRequest {
    requests: UnboundedSender<PhoneFilesRequest>,
    storage: SharedStorage,
}

impl PhoneFilesOnRequest {
    /// What asks for the phone's files, kept from WaOS as `storage` says, and where its requests
    /// arrive on the main thread.
    pub(crate) fn new(
        storage: SharedStorage,
    ) -> (PhoneFilesOnRequest, UnboundedReceiver<PhoneFilesRequest>) {
        let (requests, arriving) = mpsc::unbounded();
        (PhoneFilesOnRequest { requests, storage }, arriving)
    }

    /// Whether WaOS may reach the phone's files now.
    pub(crate) fn reachable(&self) -> bool {
        self.storage.reachable()
    }

    /// Asks the system to let WaOS reach the phone's files, for `program`, and resolves to whether
    /// it may once its user is back; as it stands where nobody answers.
    pub(crate) fn request(
        &self,
        program: &AppInstance,
    ) -> impl Future<Output = bool> + Send + use<> {
        let (reachable, answered) = oneshot::channel();
        let request = PhoneFilesRequest {
            program: program.clone(),
            reachable,
        };
        if self.requests.unbounded_send(request).is_err() {
            tracing::info!(
                program = program.name(),
                "WaOS is quitting: no phone's files"
            );
        }
        let storage = self.storage.clone();
        async move { answered.await.unwrap_or_else(|_| storage.reachable()) }
    }
}

impl ShellSystemPermissions for PhoneFilesOnRequest {
    fn state(
        &self,
        permission: SystemPermission<'_>,
        _program: &AppInstance,
    ) -> SystemPermissionState {
        match permission {
            SystemPermission::Path(path) if self.storage.keeps_from(path) => {
                SystemPermissionState::Denied
            }
            SystemPermission::Path(_) | SystemPermission::Notifications => {
                SystemPermissionState::Allowed
            }
        }
    }

    fn request(
        &self,
        permission: SystemPermission<'_>,
        program: &AppInstance,
    ) -> SystemPermissionFuture {
        if self.state(permission, program) == SystemPermissionState::Allowed {
            return Box::pin(ready(SystemPermissionState::Allowed));
        }
        let reachable = self.request(program);
        Box::pin(async move {
            if reachable.await {
                SystemPermissionState::Allowed
            } else {
                SystemPermissionState::Denied
            }
        })
    }
}

/// Whether a program's picker of machines, answered `picked`, picks This computer while WaOS may
/// not reach the phone's files, so the system is asked for them first.
pub(crate) fn asks_for_phone_files(picked: Option<&str>, reachable: bool) -> bool {
    picked == Some(THIS_COMPUTER) && !reachable
}

/// A phone of the tests' own, whose files WaOS may reach once the user allows it on the page it
/// opens, which counts its openings.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestPhone {
    pub(crate) reachable: std::cell::Cell<bool>,
    pages_opened: std::cell::Cell<usize>,
}

#[cfg(test)]
impl TestPhone {
    /// The phone's shared storage, This computer's home while WaOS may reach it.
    pub(crate) const STORAGE: &str = "/storage/emulated/0";
    /// WaOS's own folder, This computer's home before.
    pub(crate) const OWN_FOLDER: &str = "/data/user/0/dev.waos.app/files";

    pub(crate) fn reachable(reachable: bool) -> TestPhone {
        let phone = TestPhone::default();
        phone.reachable.set(reachable);
        phone
    }

    pub(crate) fn pages_opened(&self) -> usize {
        self.pages_opened.get()
    }
}

#[cfg(test)]
impl PhoneFilesAccess for TestPhone {
    fn reachable(&self) -> bool {
        self.reachable.get()
    }

    fn storage(&self) -> PathBuf {
        PathBuf::from(TestPhone::STORAGE)
    }

    fn home(&self) -> PathBuf {
        PathBuf::from(if self.reachable.get() {
            TestPhone::STORAGE
        } else {
            TestPhone::OWN_FOLDER
        })
    }

    fn open_switch(&self) -> Result<(), String> {
        self.pages_opened.set(self.pages_opened.get() + 1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_computer_picked_while_the_phone_s_files_are_out_of_reach_asks() {
        assert!(asks_for_phone_files(Some(THIS_COMPUTER), false));
        assert!(!asks_for_phone_files(Some(THIS_COMPUTER), true));
        assert!(!asks_for_phone_files(Some("build-server"), false));
        assert!(!asks_for_phone_files(None, false));
    }

    #[test]
    fn the_system_keeps_waos_from_the_phone_s_storage_until_it_may_reach_it() {
        let storage = SharedStorage::at(PathBuf::from(TestPhone::STORAGE));
        let (music, own) = (
            Path::new("/storage/emulated/0/Music"),
            Path::new(TestPhone::OWN_FOLDER),
        );
        assert!(storage.keeps_from(music));
        assert!(!storage.keeps_from(own), "WaOS's own folder is its own");
        storage.clone().set_reachable(true);
        assert!(!storage.keeps_from(music), "every clone reads what was set");
    }

    #[test]
    fn every_clone_of_this_computer_s_home_reads_what_was_set_last() {
        let home = ThisComputerHome::new(PathBuf::from(TestPhone::OWN_FOLDER));
        let read_elsewhere = home.clone();
        home.set(PathBuf::from(TestPhone::STORAGE));
        assert_eq!(read_elsewhere.get(), PathBuf::from(TestPhone::STORAGE));
    }
}
