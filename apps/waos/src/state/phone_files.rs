//! The phone's files, as the main thread last read them from the system: whether WaOS may reach
//! them, which the global settings and a pick of This computer show, and This computer's home,
//! which the programs' commands start in. Read as WaOS starts and again each time it comes back to
//! the front, where the user comes back from the system's page for them (`logic::phone_files`).
//! A request for them, for something a program's user gave it, opens that page and is answered at
//! the next read: that WaOS may reach them, or, once the user was told they are still off, that it
//! may not.
//!
//! The invariant: the system is asked here alone, on the main thread; what was read is told
//! (`ReadAgain`), and a request answered, only once This computer's home is set from it, so a pick
//! answered then starts its commands in the home the user just allowed; and every request taken
//! is answered once.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waft::{App, AppLifecycleListener, Context, Entity, EventEmitter};

use crate::logic::phone_files::{
    PhoneFilesAccess, PhoneFilesOnRequest, PhoneFilesRequest, SharedStorage, ThisComputerHome,
};
use crate::logic::prompts::PhoneFilesOffNotice;

/// Whether WaOS may reach the phone's files, as last read.
pub(crate) struct PhoneFiles {
    access: Rc<dyn PhoneFilesAccess>,
    /// What the programs' commands start in.
    home: ThisComputerHome,
    /// Where the phone's shared storage is and whether WaOS may reach it, as last read, which the
    /// other threads read too.
    storage: SharedStorage,
    /// The requests waiting for the user to come back from the system's page.
    waiting: Vec<PhoneFilesRequest>,
    /// What tells a program's user that they are still off.
    off_notice: PhoneFilesOffNotice,
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
        for request in std::mem::take(&mut self.waiting) {
            self.answer(request, cx);
        }
    }

    /// A program's user gave it something that needs the phone's files: the system's page for
    /// them opens, and the request is answered once they are read again, as the user comes back;
    /// at once where WaOS may reach them already, or the page does not open.
    fn requested(&mut self, request: PhoneFilesRequest, cx: &mut Context<PhoneFiles>) {
        if self.reachable() {
            return self.answer(request, cx);
        }
        match self.access.open_switch() {
            Ok(()) => self.waiting.push(request),
            Err(why) => {
                tracing::warn!(why, "the phone's files' page did not open");
                self.answer(request, cx);
            }
        }
    }

    /// Answers `request` as the phone's files stand: that WaOS may reach them, or, once the
    /// program's user was told they are still off, that it may not.
    fn answer(&self, request: PhoneFilesRequest, cx: &mut Context<PhoneFiles>) {
        let PhoneFilesRequest { program, reachable } = request;
        if self.reachable() {
            let _ = reachable.send(true);
            return;
        }
        let told = self.off_notice.tell(&program);
        cx.spawn(async move |_cx| {
            told.await;
            let _ = reachable.send(false);
        });
    }
}

/// What the phone's files are followed with: where the other threads read them, where the
/// requests for them arrive, and what tells a program's user that they are still off.
pub(crate) struct PhoneFilesFollowed {
    pub(crate) storage: SharedStorage,
    pub(crate) requests: UnboundedReceiver<PhoneFilesRequest>,
    pub(crate) off_notice: PhoneFilesOffNotice,
}

/// What asks for the phone's files `access` reads, from any thread, and what they are followed
/// with on the main thread; a program's user is told that they are still off by `off_notice`.
pub(crate) fn phone_files_on_request(
    access: &dyn PhoneFilesAccess,
    off_notice: PhoneFilesOffNotice,
) -> (PhoneFilesOnRequest, PhoneFilesFollowed) {
    let storage = SharedStorage::at(access.storage());
    let (on_request, requests) = PhoneFilesOnRequest::new(storage.clone());
    let followed = PhoneFilesFollowed {
        storage,
        requests,
        off_notice,
    };
    (on_request, followed)
}

/// The phone's files as `access` says, read now and again each time WaOS comes back to the front,
/// each request for them taken from now on.
pub(crate) fn follow_phone_files(
    app: &mut App,
    access: Rc<dyn PhoneFilesAccess>,
    followed: PhoneFilesFollowed,
) -> Entity<PhoneFiles> {
    let PhoneFilesFollowed {
        storage,
        requests,
        off_notice,
    } = followed;
    let phone_files = app.new_entity(|cx| {
        let mut phone_files = PhoneFiles {
            access,
            home: ThisComputerHome::new(PathBuf::new()),
            storage,
            waiting: Vec::new(),
            off_notice,
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
    take_requests(app, phone_files.clone(), requests);
    phone_files
}

/// One task takes every request for the phone's files, on the main thread.
fn take_requests(
    app: &mut App,
    phone_files: Entity<PhoneFiles>,
    mut requests: UnboundedReceiver<PhoneFilesRequest>,
) {
    app.spawn(async move |cx| {
        while let Some(request) = requests.next().await {
            cx.update(|app| {
                phone_files.update(app, |phone_files, cx| phone_files.requested(request, cx));
            });
        }
    });
}
