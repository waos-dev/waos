//! The library's side of the phone's files, where WaOS reaches them only as the user allows: the
//! global settings say whether it may, as `state::phone_files` last read it, shown again at each
//! read, and open the system's page where the user changes it; and a page that lists a folder on
//! the phone's storage says so while WaOS may not reach it.

use std::path::Path;

use waft::{App, Context, Entity};

use super::Library;
use crate::state::phone_files::PhoneFiles;

impl Library {
    /// Whether WaOS may reach the phone's files, as last read; none where it reaches what its user
    /// does.
    pub(crate) fn phone_files_reachable(&self, app: &App) -> Option<bool> {
        let phone_files = self.phone_files.as_ref()?;
        Some(phone_files.read(app).reachable())
    }

    /// Whether the system keeps WaOS from `path`, a folder the user gave a program: one on the
    /// phone's storage while WaOS may not reach it; never where it reaches what its user does.
    pub(crate) fn system_keeps_from(&self, path: &Path, app: &App) -> bool {
        (self.phone_files.as_ref())
            .is_some_and(|phone_files| phone_files.read(app).keeps_from(path))
    }

    /// Opens the system's page where the user lets WaOS reach the phone's files; one that does not
    /// open is the library's problem.
    pub(crate) fn open_phone_files_page(&mut self, cx: &mut Context<Library>) {
        let Some(phone_files) = &self.phone_files else {
            return;
        };
        if let Err(why) = phone_files.read(cx).open_switch() {
            self.failed(format!("Android's settings did not open: {why}"), cx);
        }
    }
}

/// From now on `library` says whether WaOS may reach the phone's files, as `phone_files` reads
/// them, shown again at each read.
pub(crate) fn show_phone_files(
    app: &mut App,
    library: &Entity<Library>,
    phone_files: Entity<PhoneFiles>,
) {
    library.update(app, |library, cx| {
        cx.observe(&phone_files, |_, _, cx| cx.notify()).detach();
        library.phone_files = Some(phone_files);
        cx.notify();
    });
}
