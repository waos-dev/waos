//! The programs' file dialogs on the main thread, over Waft's: each shown as a sheet on the
//! window of the program's view it names, and answered with the paths the user chose, which the
//! engine mounts. The dialogs are the engine's shell's only where Waft's platform has them.
//!
//! The invariant: every dialog arriving is answered, with the paths chosen or with why none were,
//! so no program waits for an answer that cannot come.

use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waft::ui::{Platform, ViewId};
use waft::{App, Entity};
use waos::ShellFileDialogs;
use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos_program_window::{picked_file_paths, picked_folder_path};

use crate::logic::file_dialog_calls::{Dialog, DialogCall, FileDialogsOnRequest};
use crate::state::ViewWindows;

/// Waft's file dialogs, served to the engine as its shell's.
pub(crate) struct FileDialogs {
    calls: FileDialogsOnRequest,
    arriving: UnboundedReceiver<DialogCall>,
}

impl FileDialogs {
    /// Waft's file dialogs on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<FileDialogs> {
        platform.file_dialogs()?;
        let (calls, arriving) = FileDialogsOnRequest::new();
        Some(FileDialogs { calls, arriving })
    }

    /// The dialogs as the engine's shell has them: each goes to the main thread, where
    /// [`serve`](Self::serve) shows it.
    pub(crate) fn shell(&self) -> Arc<dyn ShellFileDialogs> {
        Arc::new(self.calls.clone())
    }

    /// From now on, each dialog a program asks for is shown through Waft's, on the window of
    /// the program's view in `view_windows` it names.
    pub(crate) fn serve(self, app: &mut App, view_windows: Entity<ViewWindows>) {
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            while let Some(call) = arriving.next().await {
                let parent = call.parent_of();
                let DialogCall { dialog, chosen, .. } = call;
                let shown = cx.update(|app| show(app, &view_windows, parent, dialog));
                cx.update(|app| {
                    app.spawn(async move |_cx| {
                        // A program that ended meanwhile hears nothing.
                        let _ = chosen.send(shown.await);
                    })
                });
            }
        });
    }
}

impl DialogCall {
    /// The program and its view the dialog is a sheet on.
    fn parent_of(&self) -> Option<(waos::RunningProgram, u64)> {
        self.parent.map(|view| (self.program.clone(), view))
    }
}

/// Shows `dialog` through Waft's, on the window of the program's view `parent`, and answers the
/// paths chosen.
fn show(
    app: &mut App,
    view_windows: &Entity<ViewWindows>,
    parent: Option<(waos::RunningProgram, u64)>,
    dialog: Dialog,
) -> impl Future<Output = Result<Vec<PathBuf>, DialogError>> + use<> {
    let parent: Option<ViewId> = parent.and_then(|(program, view)| {
        let (waft_view, _ratio) = view_windows
            .read(app)
            .waft_view_of(app, |shown, shown_view| {
                *shown == program && shown_view.id() == view
            })?;
        Some(waft_view)
    });
    let platform = app.platform();
    let shown = platform.file_dialogs().map(|dialogs| match dialog {
        Dialog::OpenFile(options) => picked_file_paths(dialogs.open_file(parent, options)),
        Dialog::OpenFiles(options) => picked_file_paths(dialogs.open_files(parent, options)),
        Dialog::OpenFolder(options) => picked_folder_path(dialogs.open_folder(parent, options)),
        Dialog::SaveFile(options) => picked_file_paths(dialogs.save_file(parent, options)),
    });
    async move {
        let Some(shown) = shown else {
            return Err(DialogError::Failed("no file dialogs".to_owned()));
        };
        shown.await
    }
}
