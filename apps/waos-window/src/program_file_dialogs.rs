//! The program's file dialogs in its app (decisions 13 and 15 of
//! `docs/proposals/2026-10-29-macos-apps.md`): each the system's own panel, opened through the
//! window process's Waft host as a sheet on the window WaOS names, the one made for the view the
//! program named, and on its own where WaOS names none or that window is gone, as WaOS's own
//! dialogs open; showing it brings the app forward (`os::come_forward`), since the user must see
//! it, and WaOS launches the app in the background. The paths on the machine the user picked go
//! back to WaOS once it closes, and WaOS mounts them for the program: the window process decides
//! nothing of what the program may reach, the user picks and the program names no path.
//!
//! The invariant a later editor will break: every dialog WaOS asks for is answered once, with why
//! none was picked where the host has no file dialogs, so the program's call returns; and nothing
//! waits on WaOS.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::channel::oneshot;
use waft::ui::ViewId;
use waft::{App, Entity};
use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos::bindings::wasdk::platform::views::RequestId;
use waos_program_window::{
    PickedPaths, ViewWindows, picked_file_paths, picked_folder_path, waft_folder_options,
    waft_open_options, waft_save_options,
};
use waos_window_messages::{FileDialog, FileDialogId, FromWindow};

use crate::connection::Connection;
use crate::os::come_forward;
use crate::program_link::WaosProgram;

/// The file dialogs currently open in the window process, so WaOS can close one if its wait is
/// dropped (e.g. a setting answers it).
#[derive(Clone, Default)]
pub(crate) struct OpenFileDialogs(Rc<RefCell<HashMap<FileDialogId, oneshot::Sender<()>>>>);

impl OpenFileDialogs {
    fn insert(&self, dialog: FileDialogId, cancel: oneshot::Sender<()>) {
        self.0.borrow_mut().insert(dialog, cancel);
    }

    fn remove(&self, dialog: FileDialogId) {
        self.0.borrow_mut().remove(&dialog);
    }

    /// Closes `dialog` if it is still open, dropping its Waft dialog future without sending
    /// `FileDialogClosed`.
    pub(crate) fn close(&self, dialog: FileDialogId) {
        if let Some(cancel) = self.0.borrow_mut().remove(&dialog) {
            let _ = cancel.send(());
        }
    }
}

/// A file dialog WaOS asks the window process to open.
pub(crate) struct AskedFileDialog {
    /// What its answer names.
    pub(crate) dialog: FileDialogId,
    /// The window it is a sheet on, by the request it was made for; none for a dialog on its own.
    pub(crate) parent: Option<RequestId>,
    pub(crate) kind: FileDialog,
}

/// Opens `asked`, a sheet on the window it names among `view_windows` or on its own, the app
/// brought forward as it shows, and answers WaOS through `connection` once it closes.
pub(crate) fn open_file_dialog(
    app: &mut App,
    view_windows: &Entity<ViewWindows<WaosProgram>>,
    open_dialogs: &OpenFileDialogs,
    connection: Rc<Connection>,
    asked: AskedFileDialog,
) {
    let parent = sheet_parent(app, view_windows, asked.parent);
    let shown = show(app, parent, asked.kind);
    if shown.is_ok() {
        come_forward();
    }
    answer_when_closed(app, open_dialogs, connection, asked.dialog, shown);
}

/// Waft's view in the window made for `window` among `view_windows`, which a dialog is a sheet
/// on; none for a dialog on its own, or where that window is gone.
fn sheet_parent(
    app: &App,
    view_windows: &Entity<ViewWindows<WaosProgram>>,
    window: Option<RequestId>,
) -> Option<ViewId> {
    let window = window?;
    let (view, _ratio) = view_windows
        .read(app)
        .waft_view_of(app, |_program, view| view.window() == window)?;
    Some(view)
}

/// `kind`, shown through the host's file dialogs, a sheet on Waft's view `parent` where one is
/// given; answers the paths picked once it closes; why not, where the host has no file dialogs.
fn show(app: &App, parent: Option<ViewId>, kind: FileDialog) -> Result<PickedPaths, DialogError> {
    let platform = app.platform();
    let dialogs = platform.file_dialogs().ok_or_else(|| {
        DialogError::Failed("Waft's platform has no file dialogs to show".to_owned())
    })?;
    Ok(match kind {
        FileDialog::OpenFile(options) => {
            picked_file_paths(dialogs.open_file(parent, waft_open_options(options)))
        }
        FileDialog::OpenFiles(options) => {
            picked_file_paths(dialogs.open_files(parent, waft_open_options(options)))
        }
        FileDialog::OpenFolder(options) => {
            picked_folder_path(dialogs.open_folder(parent, waft_folder_options(options)))
        }
        FileDialog::SaveFile(options) => {
            picked_file_paths(dialogs.save_file(parent, waft_save_options(options)))
        }
    })
}

/// Answers WaOS, through `connection`, the paths picked in the dialog `dialog` once `shown`
/// closes; at once with why when none was shown; or drops `shown` without answering if
/// `open_dialogs` closes it first.
fn answer_when_closed(
    app: &mut App,
    open_dialogs: &OpenFileDialogs,
    connection: Rc<Connection>,
    dialog: FileDialogId,
    shown: Result<PickedPaths, DialogError>,
) {
    let shown = match shown {
        Ok(shown) => shown,
        Err(error) => {
            let picked = Err(error);
            connection.send(FromWindow::FileDialogClosed { dialog, picked });
            return;
        }
    };
    let (cancel, cancelled) = oneshot::channel();
    open_dialogs.insert(dialog, cancel);
    let open_dialogs = open_dialogs.clone();
    app.spawn(async move |_cx| {
        let mut shown = std::pin::pin!(shown);
        let picked = match futures::future::select(shown.as_mut(), cancelled).await {
            futures::future::Either::Left((picked, _)) => Some(picked),
            futures::future::Either::Right((_, _)) => None,
        };
        open_dialogs.remove(dialog);
        if let Some(picked) = picked {
            connection.send(FromWindow::FileDialogClosed { dialog, picked });
        }
    });
}

#[cfg(test)]
mod tests;
