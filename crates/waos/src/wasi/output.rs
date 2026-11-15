//! What a program writes to its standard output and error: to the output chosen for it in its
//! environment, as it writes it; with none, as `tracing` events naming the program, one per
//! line, `info` for stdout and `warn` for stderr, under [`targets::PROGRAM`].
//!
//! Every write is told through the program's access too, which sends it to whoever listens to the
//! program's output (`access`'s trace), and to nobody while nobody does.
//!
//! The invariants: a chosen output gets every write whole and in order, bytes as the program
//! wrote them, never cut into lines; the log gets a line as one event, whole, however the
//! program's writes split it, and what is left without a newline when the stream ends as an
//! event of its own.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::AsyncWrite;
use waos_capabilities::{OutputStream, ProgramOutput};
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};

use crate::access::ProgramAccess;
use crate::targets;

/// One of the program's standard streams, as WASI opens it: to `output` when one was chosen,
/// to the log otherwise.
pub(super) struct StandardStream {
    program: Arc<str>,
    stream: OutputStream,
    output: Option<Arc<dyn ProgramOutput>>,
    /// Where each write is told.
    access: ProgramAccess,
}

impl StandardStream {
    pub(super) fn new(
        program: &str,
        stream: OutputStream,
        output: Option<Arc<dyn ProgramOutput>>,
        access: ProgramAccess,
    ) -> StandardStream {
        StandardStream {
            program: Arc::from(program),
            stream,
            output,
            access,
        }
    }
}

impl IsTerminal for StandardStream {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for StandardStream {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        let written: Box<dyn AsyncWrite + Send + Sync + Unpin> = match &self.output {
            Some(output) => Box::new(Forwarded {
                stream: self.stream,
                output: Arc::clone(output),
            }),
            None => Box::new(Lines {
                program: Arc::clone(&self.program),
                stream: self.stream,
                partial: Vec::new(),
            }),
        };
        Box::new(Told {
            written,
            errors: matches!(self.stream, OutputStream::Stderr),
            access: self.access.clone(),
        })
    }
}

/// What a stream writes, told through the program's access as it is written.
struct Told {
    written: Box<dyn AsyncWrite + Send + Sync + Unpin>,
    errors: bool,
    access: ProgramAccess,
}

impl AsyncWrite for Told {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let polled = Pin::new(&mut self.written).poll_write(cx, bytes);
        if let Poll::Ready(Ok(written)) = &polled {
            self.access.wrote(self.errors, &bytes[..*written]);
        }
        polled
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.written).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.written).poll_shutdown(cx)
    }
}

/// The stream's bytes, handed to the chosen output as each write comes.
struct Forwarded {
    stream: OutputStream,
    output: Arc<dyn ProgramOutput>,
}

impl AsyncWrite for Forwarded {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.output.write(self.stream, bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// The stream's bytes, cut into lines for the log: an event per line, and what follows the
/// last newline kept for the next write.
struct Lines {
    program: Arc<str>,
    stream: OutputStream,
    partial: Vec<u8>,
}

impl Lines {
    fn write(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while let Some(newline) = rest.iter().position(|byte| *byte == b'\n') {
            self.partial.extend_from_slice(&rest[..newline]);
            self.record_partial();
            rest = &rest[newline + 1..];
        }
        self.partial.extend_from_slice(rest);
    }

    /// Records what is kept as a line, when there is anything.
    fn record_partial(&mut self) {
        if self.partial.is_empty() {
            return;
        }
        let line = String::from_utf8_lossy(&self.partial);
        match self.stream {
            OutputStream::Stdout => {
                tracing::info!(target: targets::PROGRAM, program = %self.program, "{line}");
            }
            OutputStream::Stderr => {
                tracing::warn!(target: targets::PROGRAM, program = %self.program, "{line}");
            }
        }
        self.partial.clear();
    }
}

impl AsyncWrite for Lines {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.write(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.record_partial();
        Poll::Ready(Ok(()))
    }
}

impl Drop for Lines {
    /// A last line without its newline is still the program's.
    fn drop(&mut self) {
        self.record_partial();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tokio::io::AsyncWriteExt;

    use super::*;

    /// An output that keeps every write as it came.
    #[derive(Default)]
    struct Recording {
        writes: Mutex<Vec<(OutputStream, Vec<u8>)>>,
    }

    impl ProgramOutput for Recording {
        fn write(&self, stream: OutputStream, bytes: &[u8]) {
            self.writes.lock().unwrap().push((stream, bytes.to_vec()));
        }
    }

    #[tokio::test]
    async fn a_chosen_output_gets_each_write_as_it_came() {
        let recording = Arc::new(Recording::default());
        let output: Arc<dyn ProgramOutput> = Arc::clone(&recording) as Arc<dyn ProgramOutput>;
        let access = ProgramAccess::new(Arc::new(crate::permissions::NothingGranted));
        let stdout = StandardStream::new(
            "test",
            OutputStream::Stdout,
            Some(Arc::clone(&output)),
            access.clone(),
        );
        let stderr = StandardStream::new("test", OutputStream::Stderr, Some(output), access);
        let mut out = Box::into_pin(stdout.async_stream());
        let mut err = Box::into_pin(stderr.async_stream());
        out.write_all(b"a prompt without its newline: ")
            .await
            .unwrap();
        err.write_all(b"a warning\n").await.unwrap();
        out.write_all(b"the answer\nand half a ").await.unwrap();
        drop((out, err));
        assert_eq!(
            *recording.writes.lock().unwrap(),
            [
                (
                    OutputStream::Stdout,
                    b"a prompt without its newline: ".to_vec()
                ),
                (OutputStream::Stderr, b"a warning\n".to_vec()),
                (OutputStream::Stdout, b"the answer\nand half a ".to_vec()),
            ]
        );
    }
}
