//! A file read or written as a component-model stream, each read and write made in place on the
//! program's thread, so a stream is never waited on; and how it ended, which the call's future
//! answers once the stream is done.
//!
//! The invariant: a stream holds its file's handle, not the descriptor, so it outlives a
//! descriptor dropped before it; and it answers how it ended exactly once, on
//! drop at the latest.

use std::fs;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::BytesMut;
use futures::channel::oneshot;
use wasmtime::StoreContextMut;
use wasmtime::component::{Destination, Source, StreamConsumer, StreamProducer, StreamResult};

use super::bindings::wasi::filesystem::types::ErrorCode;
use crate::files::descriptor::{READ_LIMIT, append, read_into, write_at};
use crate::files::errors;

/// How a stream ended, which the program reads from the call's future.
pub(super) type Ending = oneshot::Sender<Result<(), ErrorCode>>;

/// Where a stream writes.
#[derive(Clone, Copy)]
pub(super) enum WriteAt {
    Position(u64),
    End,
}

/// A file read from a position on, to its end.
pub(super) struct FileReader {
    file: Arc<fs::File>,
    position: u64,
    ending: Option<Ending>,
}

impl FileReader {
    pub(super) fn new(file: Arc<fs::File>, position: u64, ending: Ending) -> FileReader {
        FileReader {
            file,
            position,
            ending: Some(ending),
        }
    }
}

impl<D> StreamProducer<D> for FileReader {
    type Item = u8;
    type Buffer = BytesMut;

    /// Reads into what the program asked for from the position, which moves past it; the stream
    /// ends at the file's end, or where a read fails, with the system's error.
    fn poll_produce<'a>(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        store: StoreContextMut<'a, D>,
        destination: Destination<'a, Self::Item, Self::Buffer>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let mut destination = destination.as_direct(store, READ_LIMIT);
        let buffer = destination.remaining();
        if buffer.is_empty() {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        let read = read_into(&self.file, buffer, self.position);
        Poll::Ready(Ok(match read {
            Ok(0) => end(&mut self.ending, Ok(())),
            Ok(read) => {
                destination.mark_written(read);
                self.position += read as u64;
                StreamResult::Completed
            }
            Err(error) => end(&mut self.ending, Err(failed(&error))),
        }))
    }
}

impl Drop for FileReader {
    fn drop(&mut self) {
        end(&mut self.ending, Ok(()));
    }
}

/// A file written at a position on, or at its end, with what the program sends.
pub(super) struct FileWriter {
    file: Arc<fs::File>,
    at: WriteAt,
    ending: Option<Ending>,
}

impl FileWriter {
    pub(super) fn new(file: Arc<fs::File>, at: WriteAt, ending: Ending) -> FileWriter {
        FileWriter {
            file,
            at,
            ending: Some(ending),
        }
    }

    /// Writes as much of `bytes` as the system takes in one call, and answers how much.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match &mut self.at {
            WriteAt::Position(position) => {
                let written = write_at(&self.file, bytes, *position)?;
                *position += written as u64;
                Ok(written)
            }
            WriteAt::End => append(&self.file, bytes),
        }
    }
}

impl<D> StreamConsumer<D> for FileWriter {
    type Item = u8;

    /// Writes what the program sent; the stream ends where a write fails, with the system's
    /// error.
    fn poll_consume(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        source: Source<Self::Item>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let mut source = source.as_direct(store);
        let written = self.write(source.remaining());
        Poll::Ready(Ok(match written {
            Ok(written) => {
                source.mark_read(written);
                StreamResult::Completed
            }
            Err(error) => end(&mut self.ending, Err(failed(&error))),
        }))
    }
}

impl Drop for FileWriter {
    fn drop(&mut self) {
        end(&mut self.ending, Ok(()));
    }
}

/// Answers how the stream ended, where it has not yet, and ends it.
fn end(ending: &mut Option<Ending>, how: Result<(), ErrorCode>) -> StreamResult {
    if let Some(ending) = ending.take() {
        let _ = ending.send(how);
    }
    StreamResult::Dropped
}

/// WASI's code for the system's error.
fn failed(error: &io::Error) -> ErrorCode {
    errors::ErrorCode::from(error).into()
}
