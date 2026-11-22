//! The phone's files: whether WaOS may reach the phone's shared storage by its paths, which on
//! Android is the user's to allow, as "All files access" (`MANAGE_EXTERNAL_STORAGE`), on a page of
//! Android's settings; and so This computer's home, that storage once WaOS may reach it, WaOS's own
//! folder before (the projects on machines proposal's decisions 4 and 5). WaOS asks for it as the
//! user picks This computer for a program, and its settings say whether it is on. Only Android has
//! it; elsewhere WaOS reaches what its user does.
//!
//! The invariant: what WaOS may reach is read from the system each time it is asked, never kept,
//! since the user changes it in the system's settings, where WaOS hears nothing.

use std::path::PathBuf;

use waos_local::THIS_COMPUTER;

use crate::logic::prompts::Reply;

/// The phone's shared storage, as the system lets WaOS reach it.
pub(crate) trait PhoneFiles: Send + Sync {
    /// Whether WaOS may reach the phone's shared storage now.
    fn reachable(&self) -> bool;

    /// Opens the system's page where the user lets WaOS reach the phone's files; why not, when it
    /// does not open.
    fn open_switch(&self) -> Result<(), String>;

    /// The user's home on This computer: the shared storage while WaOS may reach it, WaOS's own
    /// folder before.
    fn home(&self) -> PathBuf;
}

/// Whether `reply`, to a program's picker of machines, picks This computer while WaOS may not
/// reach the phone's files, so the user is offered the system's page first.
pub(crate) fn asks_for_phone_files(reply: Option<&Reply>, phone: &dyn PhoneFiles) -> bool {
    matches!(reply, Some(Reply::Machine(id)) if id == THIS_COMPUTER) && !phone.reachable()
}

/// A phone of the tests' own, whose files WaOS may reach once the user allows it on the page it
/// opens, which counts its openings.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestPhone {
    pub(crate) reachable: std::sync::atomic::AtomicBool,
    pub(crate) pages_opened: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl TestPhone {
    pub(crate) fn reachable(reachable: bool) -> TestPhone {
        let phone = TestPhone::default();
        phone
            .reachable
            .store(reachable, std::sync::atomic::Ordering::SeqCst);
        phone
    }

    pub(crate) fn pages_opened(&self) -> usize {
        self.pages_opened.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
impl PhoneFiles for TestPhone {
    fn reachable(&self) -> bool {
        self.reachable.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn open_switch(&self) -> Result<(), String> {
        self.pages_opened
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn home(&self) -> PathBuf {
        PathBuf::from(if PhoneFiles::reachable(self) {
            "/storage/emulated/0"
        } else {
            "/data/user/0/dev.waos.app/files"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_computer_picked_while_the_phone_s_files_are_out_of_reach_asks() {
        let (out_of_reach, reachable) = (TestPhone::reachable(false), TestPhone::reachable(true));
        let this_computer = Reply::Machine(THIS_COMPUTER.to_owned());
        let other = Reply::Machine("build-server".to_owned());
        assert!(asks_for_phone_files(Some(&this_computer), &out_of_reach));
        assert!(!asks_for_phone_files(Some(&this_computer), &reachable));
        assert!(!asks_for_phone_files(Some(&other), &out_of_reach));
        assert!(!asks_for_phone_files(None, &out_of_reach));
    }
}
