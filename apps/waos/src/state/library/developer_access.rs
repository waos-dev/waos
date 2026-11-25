//! The library's side of developer access: whether computers on the network may reach WaOS's dev
//! server, kept in WaOS's preferences and followed by the dev server, and whether it listens, shown
//! again each time that changes.
//!
//! The invariant a later editor will break: access is on only once the preferences keep it on,
//! so a WaOS started again opens to the network exactly when the user last left it open.

use waft::{App, Context, Entity};

use super::{Library, follow};
use crate::logic::developer_access::{DeveloperAccess, Listening};

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
        library.developer_access = Some(access);
        cx.notify();
    });
}
