//! One paste's answer, between the program and whoever pasted: the content's chunks as the
//! program streams them, and how producing it ended, the program's `done` (decision 16 of
//! `docs/proposals/2026-10-25-clipboard.md`). The program's side is [`Answering`], which its
//! `provide` takes; the paste's is [`Answer`], which a form takes whole and a file writes into the
//! shell's sink as it comes.
//!
//! The invariant: an answer counts only once its stream has ended and `done` says ok, so a
//! producer that breaks half-way, or a request dropped unanswered, fails the paste.

use std::io;

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};

use crate::shell::FileSink;
use crate::stream::{Chunks, Settled};

/// How many chunks a program may give ahead of the paste taking them.
const CHUNKS_AHEAD: usize = 4;

/// How producing the content ended, as the program's `done` says.
pub(crate) type Done = Result<(), String>;

/// The program's side of one answer, until its `provide` takes it.
pub(crate) struct Answering {
    begun: oneshot::Sender<()>,
    chunks: mpsc::Sender<Vec<u8>>,
    done: oneshot::Sender<Done>,
}

/// The paste's side of one answer.
pub(crate) struct Answer {
    begun: oneshot::Receiver<()>,
    chunks: mpsc::Receiver<Vec<u8>>,
    done: oneshot::Receiver<Done>,
}

/// One answer's two sides.
pub(crate) fn answer() -> (Answering, Answer) {
    let (begin, begun) = oneshot::channel();
    let (chunk, chunks) = mpsc::channel(CHUNKS_AHEAD);
    let (settle, done) = oneshot::channel();
    let answering = Answering {
        begun: begin,
        chunks: chunk,
        done: settle,
    };
    (
        answering,
        Answer {
            begun,
            chunks,
            done,
        },
    )
}

impl Answering {
    /// The program answers: what its content stream and its `done` are piped into.
    pub(crate) fn begin(self) -> (Chunks, Settled<Done>) {
        let _ = self.begun.send(());
        (Chunks::new(self.chunks), Settled::new(self.done))
    }
}

impl Answer {
    /// Resolves once the program has begun to answer: false when it dropped the request instead.
    pub(crate) async fn begun(&mut self) -> bool {
        (&mut self.begun).await.is_ok()
    }

    /// The whole content, once its stream has ended and `done` says ok.
    pub(crate) async fn whole(self) -> Option<Vec<u8>> {
        let bytes: Vec<u8> = self.chunks.concat().await;
        matches!(self.done.await, Ok(Ok(()))).then_some(bytes)
    }

    /// Writes the content into `sink` as it comes; fails unless `done` says ok once its stream
    /// has ended.
    pub(crate) async fn written_into(self, sink: &mut dyn FileSink) -> io::Result<()> {
        let Answer {
            mut chunks, done, ..
        } = self;
        while let Some(chunk) = chunks.next().await {
            sink.write(&chunk).await?;
        }
        match done.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(reason)) => Err(io::Error::other(reason)),
            Err(_dropped) => Err(io::Error::other(
                "the program did not say the file was whole",
            )),
        }
    }
}

#[cfg(test)]
impl Answering {
    /// Answers as a program would: `chunks` streamed in order, then `done`, as its `provide`'s
    /// stream and future give them, or no `done` at all, as a program that drops it unwritten.
    pub(crate) fn give(self, chunks: &[&[u8]], done: Option<Done>) {
        let Answering {
            begun,
            chunks: mut sender,
            done: settle,
        } = self;
        let _ = begun.send(());
        for chunk in chunks {
            sender
                .try_send(chunk.to_vec())
                .expect("room for the test's chunks");
        }
        drop(sender);
        if let Some(done) = done {
            let _ = settle.send(done);
        }
    }
}
