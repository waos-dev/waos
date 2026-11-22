//! The library's side of the phone's files, where WaOS reaches them only as the user allows: the
//! global settings say whether it may, as the system says each time they are shown, and open the
//! system's page where the user changes it; the page is shown again each time WaOS comes back to
//! the front, since the system says nothing when the user changes it there
//! (`logic::phone_files`).

use std::rc::Rc;
use std::sync::Arc;

use waft::{App, AppLifecycleListener, Context, Entity};

use super::Library;
use crate::logic::phone_files::PhoneFiles;

impl Library {
    /// Whether WaOS may reach the phone's files now; none where it reaches what its user does.
    pub(crate) fn phone_files_reachable(&self) -> Option<bool> {
        let phone_files = self.phone_files.as_ref()?;
        Some(phone_files.reachable())
    }

    /// Opens the system's page where the user lets WaOS reach the phone's files; one that does not
    /// open is the library's problem.
    pub(crate) fn open_phone_files_page(&mut self, cx: &mut Context<Library>) {
        let Some(phone_files) = &self.phone_files else {
            return;
        };
        if let Err(why) = phone_files.open_switch() {
            self.failed(format!("Android's settings did not open: {why}"), cx);
        }
    }
}

/// From now on `library` says whether WaOS may reach the phone's files, as `phone_files` reads
/// it, read again each time WaOS comes back to the front.
pub(crate) fn show_phone_files(
    app: &mut App,
    library: &Entity<Library>,
    phone_files: Arc<dyn PhoneFiles>,
) {
    library.update(app, |library, cx| {
        library.phone_files = Some(phone_files);
        cx.notify();
    });
    let shown = library.downgrade();
    let read_again = move |app: &mut App| {
        if let Some(library) = shown.upgrade() {
            library.update(app, |_, cx| cx.notify());
        }
    };
    AppLifecycleListener::new(app).on_resume(app, Rc::new(read_again));
}
