//! The shell's file dialogs, as the platform's would answer them: each answered at once with the
//! paths a test gave for it, in order, and kept as it was asked.

use std::collections::VecDeque;
use std::future::ready;
use std::path::PathBuf;
use std::sync::Mutex;

use waos::bindings::wasdk::platform::views::ViewId;
use waos::{
    FileDialogFuture, FolderOptions, OpenOptions, RunningProgram, SaveOptions, ShellFileDialogs,
};

/// The dialogs asked for, and the answers to give the next ones.
#[derive(Default)]
pub struct TestFileDialogs {
    answers: Mutex<VecDeque<Vec<PathBuf>>>,
    asked: Mutex<Vec<AskedDialog>>,
}

/// One dialog as a program asked for it.
#[derive(Clone, Debug, PartialEq)]
pub struct AskedDialog {
    /// `open-file`, `open-files`, `open-folder` or `save-file`.
    pub kind: &'static str,
    pub parent: Option<ViewId>,
    pub initial_folder: Option<PathBuf>,
    pub suggested_name: Option<String>,
}

impl TestFileDialogs {
    /// The user chooses `paths` in the next dialog; none cancels it.
    pub fn answer_next(&self, paths: Vec<PathBuf>) {
        self.answers.lock().unwrap().push_back(paths);
    }

    /// Each dialog asked for, in order.
    pub fn asked(&self) -> Vec<AskedDialog> {
        self.asked.lock().unwrap().clone()
    }

    /// Keeps the dialog `asked`, and answers it with the next paths given.
    fn answer(&self, asked: AskedDialog) -> Vec<PathBuf> {
        self.asked.lock().unwrap().push(asked);
        self.answers.lock().unwrap().pop_front().unwrap_or_default()
    }
}

impl ShellFileDialogs for TestFileDialogs {
    fn open_file(
        &self,
        _program: &RunningProgram,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let asked = asked("open-file", parent, options.initial_folder, None);
        Box::pin(ready(Ok(self.answer(asked).into_iter().next())))
    }

    fn open_files(
        &self,
        _program: &RunningProgram,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Vec<PathBuf>> {
        let asked = asked("open-files", parent, options.initial_folder, None);
        Box::pin(ready(Ok(self.answer(asked))))
    }

    fn open_folder(
        &self,
        _program: &RunningProgram,
        parent: Option<ViewId>,
        options: FolderOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let asked = asked("open-folder", parent, options.initial_folder, None);
        Box::pin(ready(Ok(self.answer(asked).into_iter().next())))
    }

    fn save_file(
        &self,
        _program: &RunningProgram,
        parent: Option<ViewId>,
        options: SaveOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let asked = asked(
            "save-file",
            parent,
            options.initial_folder,
            options.suggested_name,
        );
        Box::pin(ready(Ok(self.answer(asked).into_iter().next())))
    }
}

fn asked(
    kind: &'static str,
    parent: Option<ViewId>,
    initial_folder: Option<PathBuf>,
    suggested_name: Option<String>,
) -> AskedDialog {
    AskedDialog {
        kind,
        parent,
        initial_folder,
        suggested_name,
    }
}
