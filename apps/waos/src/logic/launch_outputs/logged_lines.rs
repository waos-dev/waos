//! What a program writes, as WaOS's log has it whatever else hears it: an event per line under the
//! engine's `program` target, naming the program, `info` for stdout and `warn` for stderr, as the
//! engine logs a program with no output of its own.
//!
//! The invariant: a line is one event, whole, however the program's writes split it, and what is
//! left without a newline when the run ends is an event of its own.

use waos::targets;
use waos_dev_protocol::OutputStream;

/// The lines of one program's two streams, as they are written.
pub(super) struct LoggedLines {
    program: String,
    /// What each stream wrote after its last newline.
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl LoggedLines {
    pub(super) fn of(program: &str) -> LoggedLines {
        LoggedLines {
            program: program.to_owned(),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    /// Logs each line `bytes` ends, written to `stream`, and keeps what follows the last.
    pub(super) fn wrote(&mut self, stream: OutputStream, bytes: &[u8]) {
        let mut rest = bytes;
        while let Some(newline) = rest.iter().position(|byte| *byte == b'\n') {
            self.partial(stream).extend_from_slice(&rest[..newline]);
            self.log_partial(stream);
            rest = &rest[newline + 1..];
        }
        self.partial(stream).extend_from_slice(rest);
    }

    /// Logs what each stream left without a newline: the run ended.
    pub(super) fn finish(&mut self) {
        self.log_partial(OutputStream::Stdout);
        self.log_partial(OutputStream::Stderr);
    }

    fn partial(&mut self, stream: OutputStream) -> &mut Vec<u8> {
        match stream {
            OutputStream::Stdout => &mut self.stdout,
            OutputStream::Stderr | OutputStream::Unknown => &mut self.stderr,
        }
    }

    /// Logs what `stream` kept as a line, when there is anything.
    fn log_partial(&mut self, stream: OutputStream) {
        let program = self.program.clone();
        let partial = self.partial(stream);
        if partial.is_empty() {
            return;
        }
        let line = String::from_utf8_lossy(partial).into_owned();
        partial.clear();
        match stream {
            OutputStream::Stdout => {
                tracing::info!(target: targets::PROGRAM, program = %program, "{line}");
            }
            OutputStream::Stderr | OutputStream::Unknown => {
                tracing::warn!(target: targets::PROGRAM, program = %program, "{line}");
            }
        }
    }
}
