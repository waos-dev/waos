//! Where the programs' openings go: each link and file to the main thread, where Waft's opener is
//! (`state::opener`). The engine calls from a program's thread once its rules allow what is opened,
//! a file by where it is on the machine, and is answered later, through the future it was given.
//!
//! The invariant: every opening asked for reaches the main thread or is answered at once, so no
//! program waits for an answer that cannot come.

use std::path::{Path, PathBuf};

use futures::FutureExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use waos::bindings::wasdk::platform::opener::OpenError;
use waos::{OpenFuture, RunningProgram, ShellOpener};

/// One opening a program asks for, on its way to the main thread.
pub(crate) struct OpeningCall {
    pub(crate) opening: Opening,
    /// Whether the system handed it on, or why not.
    pub(crate) opened: oneshot::Sender<Result<(), OpenError>>,
}

/// What is opened.
#[derive(Debug)]
pub(crate) enum Opening {
    Link(String),
    /// A file, by its path on the machine.
    File(PathBuf),
    /// A file shown in the system's file manager, by its path on the machine.
    Reveal(PathBuf),
}

/// The app's opener, as the engine reaches it: every opening goes to the main thread.
#[derive(Clone)]
pub(crate) struct OpenerOnRequest {
    calls: UnboundedSender<OpeningCall>,
}

impl OpenerOnRequest {
    /// The opener, and where its openings arrive on the main thread.
    pub(crate) fn new() -> (OpenerOnRequest, UnboundedReceiver<OpeningCall>) {
        let (calls, arriving) = mpsc::unbounded();
        (OpenerOnRequest { calls }, arriving)
    }

    /// Sends `opening`, asked for by `program`, to the main thread, and answers whether the system
    /// handed it on.
    fn ask(&self, program: &RunningProgram, opening: Opening) -> OpenFuture {
        tracing::info!(program = program.name(), ?opening, "a program opens");
        let (opened, answered) = oneshot::channel();
        if self
            .calls
            .unbounded_send(OpeningCall { opening, opened })
            .is_err()
        {
            tracing::info!("WaOS is quitting: nothing is opened");
        }
        Box::pin(answered.map(|opened| {
            opened.unwrap_or_else(|_| Err(OpenError::Failed("WaOS is quitting".to_owned())))
        }))
    }
}

impl ShellOpener for OpenerOnRequest {
    fn open_link(&self, program: &RunningProgram, link: &str) -> OpenFuture {
        self.ask(program, Opening::Link(link.to_owned()))
    }

    fn open_file(&self, program: &RunningProgram, path: &Path) -> OpenFuture {
        self.ask(program, Opening::File(path.to_owned()))
    }

    fn reveal_file(&self, program: &RunningProgram, path: &Path) -> OpenFuture {
        self.ask(program, Opening::Reveal(path.to_owned()))
    }
}
