//! The library's side of developer access: whether computers on the network may reach WaOS's dev
//! server, kept in WaOS's preferences and followed by the dev server; whether it listens; the code
//! shown for a computer to pair with, ended once its time is up; and the computers paired, each of
//! which the user may remove. Each is shown again as it changes.
//!
//! The invariant a later editor will break: access is on only once the preferences keep it on,
//! so a WaOS started again opens to the network exactly when the user last left it open.

use waft::{App, Context, Entity};
use waos_dev_protocol::PairingCode;

use super::{Library, follow};
use crate::logic::developer_access::{CODE_LIFE, DeveloperAccess, Listening, PairedComputer};

impl Library {
    /// Whether computers on the network may reach WaOS's dev server; none where WaOS offers no
    /// choice of it, as in a phone's window on a desktop.
    pub(crate) fn developer_access_on(&self) -> Option<bool> {
        self.developer_access.as_ref()?;
        Some(self.preferences.as_ref()?.chosen().developer_access)
    }

    /// Whether the dev server listens for the network now; none where WaOS offers no choice.
    pub(crate) fn developer_listening(&self) -> Option<Listening> {
        self.developer_access_on()?;
        let access = self.developer_access.as_ref()?;
        Some(access.listening().borrow().clone())
    }

    /// The code shown for a computer to pair with; none while none is, or where access is off.
    pub(crate) fn pairing_code(&self) -> Option<PairingCode> {
        self.offered_access()?.shown_code()
    }

    /// The computers paired, in the order they paired; none where WaOS offers no choice.
    pub(crate) fn paired_computers(&self) -> Option<Vec<PairedComputer>> {
        self.developer_access_on()?;
        Some(self.developer_access.as_ref()?.paired())
    }

    /// Shows a new code for a computer to pair with, for as long as a code lasts; one that cannot
    /// be made is the library's problem.
    pub(crate) fn show_pairing_code(&mut self, cx: &mut Context<Library>) {
        let Some(access) = self.offered_access().cloned() else {
            return;
        };
        match access.show_code() {
            // The code's end is the access's to say; the settings hear it as a change.
            Ok(_) => drop(self.work.spawn(async move {
                #[allow(clippy::disallowed_methods, reason = "a pairing code lasts this long")]
                tokio::time::sleep(CODE_LIFE).await;
                access.end_expired_code();
            })),
            Err(error) => self.failed(format!("no code could be made: {error}"), cx),
        }
    }

    /// Forgets the paired computer `name`, whose token WaOS refuses from now on; one that cannot
    /// be forgotten is the library's problem.
    pub(crate) fn remove_paired_computer(&mut self, name: &str, cx: &mut Context<Library>) {
        let Some(access) = &self.developer_access else {
            return;
        };
        if let Err(error) = access.remove(name) {
            self.failed(format!("{name} could not be removed: {error}"), cx);
        }
    }

    /// Developer access, where WaOS offers it and it is on.
    fn offered_access(&self) -> Option<&DeveloperAccess> {
        self.developer_access_on()
            .filter(|on| *on)
            .and(self.developer_access.as_ref())
    }

    /// Lets computers on the network reach WaOS's dev server, or no longer, as `on` says, once
    /// the preferences keep it; where they cannot, nothing changes, and that is the library's
    /// problem.
    pub(crate) fn turn_developer_access(&mut self, on: bool, cx: &mut Context<Library>) {
        if self.developer_access_on().is_none() {
            return;
        }
        self.choose_preferences(|chosen| chosen.developer_access = on, cx);
        if let (Some(kept), Some(access)) = (self.developer_access_on(), &self.developer_access) {
            access.set_on(kept);
        }
    }
}

/// From now on `library` offers developer access, `access`, where it keeps WaOS's preferences:
/// turned on as they say, and shown again each time the dev server starts or stops listening.
pub(crate) fn offer_developer_access(
    app: &mut App,
    library: &Entity<Library>,
    access: DeveloperAccess,
) {
    library.update(app, |library, cx| {
        let Some(preferences) = &library.preferences else {
            return;
        };
        access.set_on(preferences.chosen().developer_access);
        follow(access.listening(), |_, _, cx| cx.notify(), cx);
        follow(access.changes(), |_, _, cx| cx.notify(), cx);
        library.developer_access = Some(access);
        cx.notify();
    });
}
