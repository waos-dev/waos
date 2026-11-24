//! The phone's files, as the main thread last read them from the system: whether WaOS may reach
//! them, which the global settings and a pick of This computer show, and This computer's home,
//! which the programs' commands start in. Read as WaOS starts and again each time it comes back to
//! the front, where the user comes back from the system's page for them (`logic::phone_files`).
//!
//! The invariant: the system is asked here alone, on the main thread; what was read is told
//! (`ReadAgain`) only once This computer's home is set from it, so a pick answered then starts its
//! commands in the home the user just allowed.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use waft::{App, AppLifecycleListener, Context, Entity, EventEmitter};

use crate::logic::phone_files::{PhoneFilesAccess, SharedStorage, ThisComputerHome};

/// Whether WaOS may reach the phone's files, as last read.
pub(crate) struct PhoneFiles {
    access: Rc<dyn PhoneFilesAccess>,
    /// What the programs' commands start in.
    home: ThisComputerHome,
    /// Where the phone's shared storage is and whether WaOS may reach it, as last read, which the
    /// other threads read too.
    storage: SharedStorage,
}

/// The phone's files were read again: WaOS started, or came back to the front.
pub(crate) struct ReadAgain;

impl EventEmitter<ReadAgain> for PhoneFiles {}

impl PhoneFiles {
    /// Whether WaOS may reach the phone's files, as last read.
    pub(crate) fn reachable(&self) -> bool {
        self.storage.reachable()
    }

    /// Whether the system keeps WaOS from `path`: it is on the phone's own storage, which WaOS
    /// may not reach.
    pub(crate) fn keeps_from(&self, path: &Path) -> bool {
        self.storage.keeps_from(path)
    }

    /// This computer's home, which the programs' commands start in, set at each read.
    pub(crate) fn home(&self) -> ThisComputerHome {
        self.home.clone()
    }

    /// Opens the system's page where the user lets WaOS reach the phone's files; why not, when it
    /// does not open.
    pub(crate) fn open_switch(&self) -> Result<(), String> {
        self.access.open_switch()
    }

    fn read_again(&mut self, cx: &mut Context<PhoneFiles>) {
        self.storage.set_reachable(self.access.reachable());
        self.home.set(self.access.home());
        tracing::info!(reachable = self.reachable(), home = ?self.home.get(), "the phone's files");
        cx.emit(ReadAgain);
        cx.notify();
    }
}

/// The phone's files as `access` says, read now and again each time WaOS comes back to the front,
/// whether WaOS may reach them told to `storage` at each read.
pub(crate) fn follow_phone_files(
    app: &mut App,
    access: Rc<dyn PhoneFilesAccess>,
    storage: SharedStorage,
) -> Entity<PhoneFiles> {
    let phone_files = app.new_entity(|cx| {
        let mut phone_files = PhoneFiles {
            access,
            home: ThisComputerHome::new(PathBuf::new()),
            storage,
        };
        phone_files.read_again(cx);
        phone_files
    });
    let followed = phone_files.downgrade();
    let read_again = move |app: &mut App| {
        if let Some(phone_files) = followed.upgrade() {
            phone_files.update(app, |phone_files, cx| phone_files.read_again(cx));
        }
    };
    AppLifecycleListener::new(app).on_resume(app, Rc::new(read_again));
    phone_files
}
