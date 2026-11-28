//! A program's file dialogs on macOS, its app's: the engine's file dialogs, each taking the getter,
//! so a program whose app does not run has it launched, with no window, and the dialog opens there
//! on its own. The window process opens it, a sheet on the window of the view the program names, or
//! on its own where it names none or no window there shows it, as WaOS's own dialogs open, and
//! brings the app forward as it shows, WaOS yielding its activation to the app as it asks. The
//! dialog answers the paths the user picked once it closes, which the engine mounts as it mounts
//! the picks of WaOS's own dialogs: the window process is WaOS's own code, its code hash and every
//! connection's sender checked, and the program names no path itself. A dialog whose app never
//! comes, or goes while it is open, answers why nothing was picked.
//!
//! The invariant a later editor will break: every dialog asked for is answered exactly once, so no
//! program waits on a dialog nobody shows.

use std::path::PathBuf;

use futures::FutureExt;
use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos::bindings::wasdk::platform::views::ViewId;
use waos::{
    AppInstance, FileDialogFuture, FolderOptions, OpenOptions, SaveOptions, ShellFileDialogs,
};
use waos_window_messages::FileDialog;

use super::window_processes::{WindowProcesses, let_come_forward};

/// The engine's file dialogs on macOS: a program's open in its app, launched for it when it does
/// not run, a sheet on the window of its view `parent` there when one is given.
impl ShellFileDialogs for WindowProcesses {
    fn open_file(
        &self,
        program: &AppInstance,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        first_picked(open_in_app(
            self,
            program,
            parent,
            FileDialog::OpenFile(options),
        ))
    }

    fn open_files(
        &self,
        program: &AppInstance,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Vec<PathBuf>> {
        open_in_app(self, program, parent, FileDialog::OpenFiles(options))
    }

    fn open_folder(
        &self,
        program: &AppInstance,
        parent: Option<ViewId>,
        options: FolderOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        first_picked(open_in_app(
            self,
            program,
            parent,
            FileDialog::OpenFolder(options),
        ))
    }

    fn save_file(
        &self,
        program: &AppInstance,
        parent: Option<ViewId>,
        options: SaveOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        first_picked(open_in_app(
            self,
            program,
            parent,
            FileDialog::SaveFile(options),
        ))
    }
}

/// Opens the file dialog `kind` in `program`'s app as `window_processes` reach it, the app
/// brought forward as it shows; answers the paths picked in it, or why none were.
fn open_in_app(
    window_processes: &WindowProcesses,
    program: &AppInstance,
    parent: Option<ViewId>,
    kind: FileDialog,
) -> FileDialogFuture<Vec<PathBuf>> {
    let_come_forward(program);
    let app = window_processes.of_instance(program);
    Box::pin(async move {
        match app.await {
            Ok(app) => app.open_file_dialog(parent, kind).await,
            Err(why) => Err(failed(&why)),
        }
    })
}

/// The one path a dialog of a single pick answers, of those `picked` answers; none when the user
/// cancelled.
fn first_picked(picked: FileDialogFuture<Vec<PathBuf>>) -> FileDialogFuture<Option<PathBuf>> {
    Box::pin(picked.map(|picked| Ok(picked?.into_iter().next())))
}

/// A dialog that answers nothing, for `reason`.
pub(super) fn failed(reason: &str) -> DialogError {
    DialogError::Failed(reason.to_owned())
}

#[cfg(test)]
mod tests;
