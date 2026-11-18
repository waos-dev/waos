//! The shell's file dialogs, as the platform's would answer them: each answered at once with the
//! paths a test gave for it, in order, and kept as it was asked.

use std::collections::VecDeque;
use std::future::ready;
use std::path::PathBuf;
use std::sync::Mutex;

use futures::FutureExt;
use futures::channel::oneshot;
use waos::bindings::wasdk::platform::views::ViewId;
use waos::{
    FileDialogFuture, FolderOptions, OpenOptions, RunningProgram, SaveOptions, ShellFileDialogs,
};

enum NextAnswer {
    Ready(Vec<PathBuf>),
    Hold,
}

/// The dialogs asked for, and the answers to give the next ones.
#[derive(Default)]
pub struct TestFileDialogs {
    answers: Mutex<VecDeque<NextAnswer>>,
    asked: Mutex<Vec<AskedDialog>>,
    held: Mutex<Vec<oneshot::Sender<Vec<PathBuf>>>>,
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
        self.answers
            .lock()
            .unwrap()
            .push_back(NextAnswer::Ready(paths));
    }

    /// Keeps the next dialog waiting until its future is dropped (or answered via `held_cancelled`).
    pub fn hold_next(&self) {
        self.answers.lock().unwrap().push_back(NextAnswer::Hold);
    }

    /// Whether the `index`th held dialog's future was dropped without answering it.
    pub fn held_cancelled(&self, index: usize) -> bool {
        self.held
            .lock()
            .unwrap()
            .get(index)
            .is_some_and(oneshot::Sender::is_canceled)
    }

    /// Each dialog asked for, in order.
    pub fn asked(&self) -> Vec<AskedDialog> {
        self.asked.lock().unwrap().clone()
    }

    /// Keeps `asked`, and returns the next future for it.
    fn answer(&self, asked: AskedDialog) -> FileDialogFuture<Vec<PathBuf>> {
        self.asked.lock().unwrap().push(asked);
        match self.answers.lock().unwrap().pop_front() {
            Some(NextAnswer::Ready(paths)) => Box::pin(ready(Ok(paths))),
            Some(NextAnswer::Hold) => {
                let (tx, rx) = oneshot::channel();
                self.held.lock().unwrap().push(tx);
                Box::pin(rx.map(|paths| Ok(paths.unwrap_or_default())))
            }
            None => Box::pin(ready(Ok(Vec::new()))),
        }
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
        let future = self.answer(asked);
        Box::pin(future.map(|res| Ok(res?.into_iter().next())))
    }

    fn open_files(
        &self,
        _program: &RunningProgram,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Vec<PathBuf>> {
        let asked = asked("open-files", parent, options.initial_folder, None);
        self.answer(asked)
    }

    fn open_folder(
        &self,
        _program: &RunningProgram,
        parent: Option<ViewId>,
        options: FolderOptions,
    ) -> FileDialogFuture<Option<PathBuf>> {
        let asked = asked("open-folder", parent, options.initial_folder, None);
        let future = self.answer(asked);
        Box::pin(future.map(|res| Ok(res?.into_iter().next())))
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
        let future = self.answer(asked);
        Box::pin(future.map(|res| Ok(res?.into_iter().next())))
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
