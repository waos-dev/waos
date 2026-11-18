//! The programs' file dialogs, whichever shows them (WaOS's own, or on macOS each program's app),
//! each noted as asked of the program's user while it is open (`asking_now`), so a run of `cargo
//! waos run` says it is open and a tool's input waits for it rather than passing under it; and
//! answered by a setting made while it waits, as a person choosing in it would, which leaves the
//! panel or sheet showing where it was shown, since neither Waft nor the system offers a way to
//! put one away.
//!
//! The invariant: a dialog is noted from the moment it is asked for until its answer comes, so
//! nothing a tool reads says a dialog is open that has closed; and the first answer, the user's or
//! a setting's, is the dialog's, so the engine mounts one set of paths.

use std::path::PathBuf;
use std::sync::Arc;

use futures::future::{self, Either};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use waos::bindings::wasdk::platform::views::ViewId;
use waos::{
    FileDialogFuture, FolderOptions, OpenOptions, RunningProgram, SaveOptions, ShellFileDialogs,
};
use waos_dev_protocol::{Asked, FileDialog, Row};

use super::asking_now::{AnswerNow, AskingNow};

/// `dialogs`, each noted in `asking` while it is open, and answered by a setting made meanwhile.
pub(crate) struct NotedFileDialogs {
    pub(crate) dialogs: Arc<dyn ShellFileDialogs>,
    pub(crate) asking: Arc<AskingNow>,
    /// Each setting made while a dialog waits, for this to answer it with.
    pub(crate) answers: broadcast::Receiver<AnswerNow>,
}

impl NotedFileDialogs {
    /// `shown`, noted as asked until it answers, or until a setting answers `dialog` first; `one`
    /// shapes the paths a setting gives as the dialog's own answer.
    fn noted<T: Send + 'static>(
        &self,
        program: &RunningProgram,
        dialog: FileDialog,
        shown: FileDialogFuture<T>,
        one: fn(Vec<PathBuf>) -> T,
    ) -> FileDialogFuture<T> {
        let noted = self
            .asking
            .while_asked(program, Asked::FileDialog(dialog), shown);
        let setting = answered_by(self.answers.resubscribe(), program, dialog);
        Box::pin(async move {
            futures::pin_mut!(noted, setting);
            match future::select(noted, setting).await {
                Either::Left((answer, _)) => answer,
                Either::Right((paths, _)) => Ok(one(paths)),
            }
        })
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
        self.noted(program, FileDialog::OpenFile, shown, |paths| {
            paths.into_iter().next()
        })
    }

    fn open_files(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Vec<PathBuf>> {
        let shown = self.dialogs.open_files(program, parent, options);
        self.noted(program, FileDialog::OpenFiles, shown, |paths| paths)
    }

    fn open_folder(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: FolderOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let shown = self.dialogs.open_folder(program, parent, options);
        self.noted(program, FileDialog::OpenFolder, shown, |paths| {
            paths.into_iter().next()
        })
    }

    fn save_file(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: SaveOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let shown = self.dialogs.save_file(program, parent, options);
        self.noted(program, FileDialog::SaveFile, shown, |paths| {
            paths.into_iter().next()
        })
    }
}

/// The paths a setting gives `dialog` of `program`, or none for its cancel, as a person choosing in
/// the dialog would; waits while no setting answers it.
fn answered_by(
    mut answers: broadcast::Receiver<AnswerNow>,
    program: &RunningProgram,
    dialog: FileDialog,
) -> impl Future<Output = Vec<PathBuf>> + Send + 'static {
    let named = program.name().to_owned();
    let key = dialog.key();
    async move {
        loop {
            match answers.recv().await {
                Ok(answer) => {
                    if let Some(paths) = answered(answer, &named, key) {
                        return paths;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                // The app is quitting: the dialog's own answer, if one comes, decides.
                Err(RecvError::Closed) => return future::pending().await,
            }
        }
    }
}

/// The paths `answer` gives the dialog whose row's key is `key`, of the program `program`; none
/// when it answers another question, or sets this one back to ask.
fn answered(answer: AnswerNow, program: &str, key: &str) -> Option<Vec<PathBuf>> {
    match answer {
        AnswerNow::Set {
            program: named,
            row: Row::FileDialog(kind, paths),
        } if named == program && kind.key() == key && !paths.is_empty() => Some(paths),
        AnswerNow::Cancel {
            program: named,
            key: named_key,
        } if named == program && named_key == key => Some(Vec::new()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(program: &str, dialog: FileDialog, paths: &[&str]) -> AnswerNow {
        AnswerNow::Set {
            program: program.to_owned(),
            row: Row::FileDialog(dialog, paths.iter().map(PathBuf::from).collect()),
        }
    }

    #[test]
    fn a_setting_answers_the_dialog_it_names_and_no_other() {
        let saved = set("/tmp/a.txt", FileDialog::SaveFile, &["/tmp/out.pdf"]);
        assert_eq!(
            answered(saved, "/tmp/a.txt", "save-file"),
            Some(vec![PathBuf::from("/tmp/out.pdf")])
        );
        let opened = set("/tmp/a.txt", FileDialog::OpenFile, &["/tmp/in.txt"]);
        assert_eq!(answered(opened, "/tmp/a.txt", "save-file"), None);
        let other = set("/tmp/b.txt", FileDialog::SaveFile, &["/tmp/out.pdf"]);
        assert_eq!(answered(other, "/tmp/a.txt", "save-file"), None);
    }

    #[test]
    fn a_cancel_answers_with_nothing_and_a_setting_back_to_ask_does_not() {
        let cancel = AnswerNow::Cancel {
            program: "/tmp/a.txt".to_owned(),
            key: "open-files".to_owned(),
        };
        assert_eq!(
            answered(cancel, "/tmp/a.txt", "open-files"),
            Some(Vec::new())
        );
        let ask = set("/tmp/a.txt", FileDialog::OpenFiles, &[]);
        assert_eq!(answered(ask, "/tmp/a.txt", "open-files"), None);
    }
}
