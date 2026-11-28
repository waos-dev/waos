//! Where the programs' file dialogs go: each to the main thread, where Waft's dialogs are
//! (`state::file_dialogs`), with what a dialog offers in Waft's terms (`waos-program-window`'s).
//! The engine calls from a program's thread and is answered later, through the future it was
//! given, with the paths the user chose.
//!
//! The invariant: every dialog asked for reaches the main thread or is answered at once, so no
//! program waits for an answer that cannot come.

use std::path::PathBuf;

use futures::FutureExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waft::ui;
use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos::bindings::wasdk::platform::views::ViewId;
use waos::{
    AppInstance, FileDialogFuture, FolderOptions, OpenOptions, SaveOptions, ShellFileDialogs,
};
use waos_program_window::{waft_folder_options, waft_open_options, waft_save_options};

/// One dialog a program asks for, on its way to the main thread.
pub(crate) struct DialogCall {
    pub(crate) program: AppInstance,
    /// The program's view whose window the dialog is a sheet on.
    pub(crate) parent: Option<ViewId>,
    pub(crate) dialog: Dialog,
    /// The paths the user chose; none when the user cancelled.
    pub(crate) chosen: oneshot::Sender<Result<Vec<PathBuf>, DialogError>>,
}

/// A dialog of each kind, with what it offers, in Waft's terms.
pub(crate) enum Dialog {
    OpenFile(ui::OpenOptions),
    OpenFiles(ui::OpenOptions),
    OpenFolder(ui::FolderOptions),
    SaveFile(ui::SaveOptions),
}

/// The app's dialogs, as the engine reaches them: every one goes to the main thread.
#[derive(Clone)]
pub(crate) struct FileDialogsOnRequest {
    calls: UnboundedSender<DialogCall>,
}

impl FileDialogsOnRequest {
    /// The dialogs, and where they arrive on the main thread.
    pub(crate) fn new() -> (FileDialogsOnRequest, UnboundedReceiver<DialogCall>) {
        let (calls, arriving) = mpsc::unbounded();
        (FileDialogsOnRequest { calls }, arriving)
    }

    /// Sends `dialog` to the main thread, and answers the paths chosen in it.
    fn ask(
        &self,
        program: &AppInstance,
        parent: Option<ViewId>,
        dialog: Dialog,
    ) -> impl Future<Output = Result<Vec<PathBuf>, DialogError>> + Send + use<> {
        let (chosen, answered) = oneshot::channel();
        let call = DialogCall {
            program: program.clone(),
            parent,
            dialog,
            chosen,
        };
        if self.calls.unbounded_send(call).is_err() {
            tracing::info!("WaOS is quitting: a file dialog is not shown");
        }
        answered.map(|chosen| {
            chosen.unwrap_or_else(|_| Err(DialogError::Failed("WaOS is quitting".to_owned())))
        })
    }
}

impl ShellFileDialogs for FileDialogsOnRequest {
    fn open_file(
        &self,
        program: &AppInstance,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let dialog = Dialog::OpenFile(waft_open_options(options));
        let asked = self.ask(program, parent, dialog);
        Box::pin(asked.map(|chosen| Ok(chosen?.into_iter().next())))
    }

    fn open_files(
        &self,
        program: &AppInstance,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Vec<PathBuf>> {
        let dialog = Dialog::OpenFiles(waft_open_options(options));
        Box::pin(self.ask(program, parent, dialog))
    }

    fn open_folder(
        &self,
        program: &AppInstance,
        parent: Option<ViewId>,
        options: FolderOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let dialog = Dialog::OpenFolder(waft_folder_options(options));
        let asked = self.ask(program, parent, dialog);
        Box::pin(asked.map(|chosen| Ok(chosen?.into_iter().next())))
    }

    fn save_file(
        &self,
        program: &AppInstance,
        parent: Option<ViewId>,
        options: SaveOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let dialog = Dialog::SaveFile(waft_save_options(options));
        let asked = self.ask(program, parent, dialog);
        Box::pin(asked.map(|chosen| Ok(chosen?.into_iter().next())))
    }
}
