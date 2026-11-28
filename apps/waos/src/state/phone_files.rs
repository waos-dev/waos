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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures::FutureExt as _;
    use waft::ui::PlatformRef;
    use waft::{AppCell, AppLifecycleState};
    use waft_services::ServicesBinding;
    use waft_test::TestPlatform;
    use waos::{AppInstance, ShellSystemPermissions, SystemPermission, SystemPermissionState};

    use super::*;
    use crate::logic::app_instances::{OpenConfiguration, OpenedFor};
    use crate::logic::phone_files::TestPhone;
    use crate::logic::prompts::{Asked, Asking, PromptsOnRequest};
    use crate::logic::startup::Setup;
    use crate::logic::startup::tests::start_with_no_windows;
    use crate::logic::test_installs;
    use crate::state::library::work;
    use crate::state::test_turns::{Turn, turn_until};

    /// The phone's files, out of WaOS's reach on a phone of the test's own, followed on an app
    /// of Waft's test platform, with an app running whose user picks on the phone's storage; the
    /// test plays that user, reading what they are told where it arrives.
    struct PhoneFilesTest {
        cell: Rc<AppCell>,
        platform: Rc<TestPlatform>,
        phone: Rc<TestPhone>,
        on_request: PhoneFilesOnRequest,
        told: UnboundedReceiver<Asked>,
        program: AppInstance,
        work: Arc<tokio::runtime::Runtime>,
        _folder: tempfile::TempDir,
    }

    impl PhoneFilesTest {
        fn new() -> PhoneFilesTest {
            let folder = tempfile::tempdir().unwrap();
            let forever = "dev.example.forever";
            let app = test_installs::built_app(waos_test_programs::FOREVER, forever, "Forever");
            let work = work().unwrap();
            let setup = Setup::in_memory(folder.path(), vec![app], work.handle().clone());
            let started = start_with_no_windows(setup);
            let configuration = OpenConfiguration::new(OpenedFor::User);
            let opened = work.block_on(
                started
                    .instances
                    .open(&forever.parse().unwrap(), configuration),
            );
            let program = opened.expect("it opens").instance().clone();
            let phone = Rc::new(TestPhone::reachable(false));
            let (prompts, prompted) = PromptsOnRequest::new();
            let (on_request, followed) =
                phone_files_on_request(phone.as_ref(), prompts.phone_files_off());
            let platform = Rc::new(TestPlatform::new());
            let cell = AppCell::with_platform(Rc::clone(&platform) as PlatformRef);
            let access = Rc::clone(&phone) as Rc<dyn PhoneFilesAccess>;
            follow_phone_files(&mut cell.borrow_mut(), access, followed);
            cell.checkpoint();
            PhoneFilesTest {
                cell,
                platform,
                phone,
                on_request,
                told: prompted.questions,
                program,
                work,
                _folder: folder,
            }
        }

        /// The user picks a file on the phone's storage, which the system keeps from WaOS.
        fn pick_on_the_storage(&self) -> waos::SystemPermissionFuture {
            let music = SystemPermission::Path(Path::new("/storage/emulated/0/Music"));
            ShellSystemPermissions::request(&self.on_request, music, &self.program)
        }

        /// Runs the app's tasks as they wake until `done`.
        fn turn_until(&self, what: &str, done: impl Fn() -> bool) {
            turn_until(&self.platform, what, || {
                self.cell.checkpoint();
                if done() { Turn::Done } else { Turn::Idle }
            });
        }

        /// The user leaves WaOS for the system's page and comes back.
        fn come_back(&self) {
            for state in [AppLifecycleState::Paused, AppLifecycleState::Resumed] {
                let mut app = self.cell.borrow_mut();
                ServicesBinding::instance(&mut app).handle_lifecycle_message(&mut app, state);
            }
            self.cell.checkpoint();
        }
    }

    impl Drop for PhoneFilesTest {
        fn drop(&mut self) {
            self.program.stop();
            self.work.block_on(self.program.ended());
        }
    }

    #[test]
    fn a_pick_out_of_reach_opens_the_systems_page_and_is_answered_as_the_user_comes_back() {
        let mut test = PhoneFilesTest::new();
        let mut picked = test.pick_on_the_storage();
        test.turn_until("the system's page opened", || {
            test.phone.pages_opened() == 1
        });
        assert_eq!(
            picked.as_mut().now_or_never(),
            None,
            "not while the page is open"
        );

        test.phone.reachable.set(true);
        test.come_back();

        assert_eq!(
            picked.as_mut().now_or_never(),
            Some(SystemPermissionState::Allowed)
        );
        assert!(test.told.try_recv().is_err(), "nothing to tell");
    }

    #[test]
    fn a_pick_still_out_of_reach_as_the_user_comes_back_is_denied_once_they_are_told() {
        let mut test = PhoneFilesTest::new();
        let mut picked = test.pick_on_the_storage();
        test.turn_until("the system's page opened", || {
            test.phone.pages_opened() == 1
        });

        test.come_back();

        let told = test
            .work
            .block_on(test.told.next())
            .expect("told they are off");
        assert!(matches!(told.asking, Asking::PhoneFilesOff { .. }));
        assert_eq!(
            picked.as_mut().now_or_never(),
            None,
            "not before it is read"
        );
        told.asking.reply(None);
        let mut state = None;
        turn_until(&test.platform, "the pick answered", || {
            test.cell.checkpoint();
            state = picked.as_mut().now_or_never();
            if state.is_some() {
                Turn::Done
            } else {
                Turn::Idle
            }
        });
        assert_eq!(state, Some(SystemPermissionState::Denied));
        assert!(test.told.try_recv().is_err(), "told once");
    }
}
