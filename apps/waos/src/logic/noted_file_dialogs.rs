//! The programs' file dialogs, whichever shows them (WaOS's own, or on macOS each program's app),
//! each noted as asked of the program's user while it is open (`asking_now`), so a run of `cargo
//! waos run` says it is open and a tool's input waits for it rather than passing under it.
//!
//! The invariant: a dialog is noted from the moment it is asked for until its answer comes, so
//! nothing a tool reads says a dialog is open that has closed.

use std::path::PathBuf;
use std::sync::Arc;

use waos::bindings::wasdk::platform::views::ViewId;
use waos::{
    FileDialogFuture, FolderOptions, OpenOptions, RunningProgram, SaveOptions, ShellFileDialogs,
};
use waos_dev_protocol::{Asked, FileDialog};

use super::asking_now::AskingNow;

/// `dialogs`, each noted in `asking` while it is open.
pub(crate) struct NotedFileDialogs {
    pub(crate) dialogs: Arc<dyn ShellFileDialogs>,
    pub(crate) asking: Arc<AskingNow>,
}

impl NotedFileDialogs {
    /// `answer` to `program`'s `dialog`, noted as asked until it comes.
    fn noted<T: Send + 'static>(
        &self,
        program: &RunningProgram,
        dialog: FileDialog,
        answer: FileDialogFuture<T>,
    ) -> FileDialogFuture<T> {
        Box::pin(
            self.asking
                .while_asked(program, Asked::FileDialog(dialog), answer),
        )
    }
}

impl ShellFileDialogs for NotedFileDialogs {
    fn open_file(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let shown = self.dialogs.open_file(program, parent, options);
        self.noted(program, FileDialog::OpenFile, shown)
    }

    fn open_files(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Vec<PathBuf>> {
        let shown = self.dialogs.open_files(program, parent, options);
        self.noted(program, FileDialog::OpenFiles, shown)
    }

    fn open_folder(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: FolderOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let shown = self.dialogs.open_folder(program, parent, options);
        self.noted(program, FileDialog::OpenFolder, shown)
    }

    fn save_file(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: SaveOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let shown = self.dialogs.save_file(program, parent, options);
        self.noted(program, FileDialog::SaveFile, shown)
    }
}
