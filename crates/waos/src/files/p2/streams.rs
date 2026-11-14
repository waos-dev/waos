//! A file read or written as a `wasi:io` stream, each read and write made in place on the
//! program's thread, so the stream is always ready.
//!
//! The invariant: a stream holds its file's handle, not the descriptor, so it outlives a
//! descriptor dropped before it, as `wasmtime-wasi`'s does.

use std::fs;
use std::io;
use std::sync::Arc;

use wasmtime::format_err;
use wasmtime_wasi_io::async_trait;
use wasmtime_wasi_io::bytes::Bytes;
use wasmtime_wasi_io::poll::Pollable;
use wasmtime_wasi_io::streams::{InputStream, OutputStream, StreamError, StreamResult};

use crate::files::descriptor::{READ_LIMIT, append, read_at, write_at};

/// How much a program may write at once: what one read takes.
const WRITE_LIMIT: usize = READ_LIMIT;

/// A file read from a position on.
pub(super) struct FileInputStream {
    file: Arc<fs::File>,
    position: u64,
}

impl FileInputStream {
    pub(super) fn new(file: Arc<fs::File>, position: u64) -> FileInputStream {
        FileInputStream { file, position }
    }
}

#[async_trait]
impl InputStream for FileInputStream {
    /// Up to `size` bytes from the position, which moves past them; the stream is closed at the
    /// file's end.
    fn read(&mut self, size: usize) -> StreamResult<Bytes> {
        if size == 0 {
            return Ok(Bytes::new());
        }
        let bytes = read_at(&self.file, size, self.position).map_err(failed)?;
        if bytes.is_empty() {
            return Err(StreamError::Closed);
        }
        self.position += bytes.len() as u64;
        Ok(Bytes::from(bytes))
    }
}

#[async_trait]
impl Pollable for FileInputStream {
    async fn ready(&mut self) {}
}

/// A file written at a position on, or at its end.
pub(super) struct FileOutputStream {
    file: Arc<fs::File>,
    at: WriteAt,
    /// Whether a write failed, after which the stream takes no more.
    closed: bool,
}

/// Where a stream writes.
enum WriteAt {
    Position(u64),
    End,
}

impl FileOutputStream {
    /// A stream writing `file` from `position` on.
    pub(super) fn at(file: Arc<fs::File>, position: u64) -> FileOutputStream {
        FileOutputStream {
            file,
            at: WriteAt::Position(position),
            closed: false,
        }
    }

    /// A stream writing at `file`'s end, wherever it is at each write.
    pub(super) fn appending(file: Arc<fs::File>) -> FileOutputStream {
        FileOutputStream {
            file,
            at: WriteAt::End,
            closed: false,
        }
    }

    /// Writes the whole of `bytes`, in as many calls as the system takes.
    fn write_all(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            let written = match &mut self.at {
                WriteAt::Position(position) => {
                    let written = write_at(&self.file, bytes, *position)?;
                    *position += written as u64;
                    written
                }
                WriteAt::End => append(&self.file, bytes)?,
            };
            bytes = &bytes[written..];
        }
        Ok(())
    }

    fn check_open(&self) -> StreamResult<()> {
        if self.closed {
            Err(StreamError::Closed)
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl OutputStream for FileOutputStream {
    /// Writes `bytes`, which may be no more than `check-write` permits: a write past it traps,
    /// as `wasi:io` says.
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        self.check_open()?;
        if bytes.len() > WRITE_LIMIT {
            return Err(StreamError::Trap(format_err!(
                "a write of {} bytes is more than check-write permits",
                bytes.len()
            )));
        }
        self.write_all(&bytes).map_err(|error| {
            self.closed = true;
            failed(error)
        })
    }

    /// Nothing is held back: each write is made when it is asked for.
    fn flush(&mut self) -> StreamResult<()> {
        self.check_open()
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        self.check_open()?;
        Ok(WRITE_LIMIT)
    }
}

#[async_trait]
impl Pollable for FileOutputStream {
    async fn ready(&mut self) {}
}

/// A read or write that failed, told to the program as the system's error, which
/// `filesystem-error-code` reads back.
fn failed(error: io::Error) -> StreamError {
    StreamError::LastOperationFailed(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_past_what_check_write_permits_traps_and_writes_nothing() {
        let file = Arc::new(tempfile::tempfile().unwrap());
        let mut stream = FileOutputStream::at(Arc::clone(&file), 0);
        let permitted = stream.check_write().unwrap();

        let past = stream.write(Bytes::from(vec![1; permitted + 1]));
        assert!(matches!(past, Err(StreamError::Trap(_))), "{past:?}");
        assert_eq!(file.metadata().unwrap().len(), 0);

        stream.write(Bytes::from(vec![1; permitted])).unwrap();
        assert_eq!(file.metadata().unwrap().len(), permitted as u64);
    }
}
