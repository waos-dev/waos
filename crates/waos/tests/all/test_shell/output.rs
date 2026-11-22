//! A program's own output, which a test reads: the lines it printed to its standard output, as
//! the engine hands them over, and a wait for a line that wakes as each is printed.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};
use std::time::Instant;

use waos::{OutputStream, ProgramOutput};

use super::WITHIN;

/// What one program printed to its standard output.
#[derive(Default)]
pub struct RecordedOutput {
    printed: Mutex<Vec<u8>>,
    /// Woken as the program prints.
    printing: Condvar,
}

impl RecordedOutput {
    /// Every line the program printed so far, in order.
    pub fn lines(&self) -> Vec<String> {
        lines_of(&self.printed())
    }

    /// Whether the program printed `line`.
    pub fn has(&self, line: &str) -> bool {
        self.count(line) > 0
    }

    /// How many times the program printed `line`.
    pub fn count(&self, line: &str) -> usize {
        self.lines()
            .iter()
            .filter(|printed| *printed == line)
            .count()
    }

    /// Waits until the program printed `line`, failing after [`WITHIN`] with what it printed.
    pub fn wait_for(&self, line: &str) {
        let deadline = Instant::now() + WITHIN;
        let mut printed = self.printed();
        while !lines_of(&printed).iter().any(|printed| printed == line) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "no {line:?} in {:#?}", lines_of(&printed));
            printed = self
                .printing
                .wait_timeout(printed, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn printed(&self) -> MutexGuard<'_, Vec<u8>> {
        self.printed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ProgramOutput for RecordedOutput {
    fn poll_write(&self, stream: OutputStream, _cx: &mut Context<'_>, bytes: &[u8]) -> Poll<usize> {
        if stream == OutputStream::Stdout {
            self.printed().extend_from_slice(bytes);
            self.printing.notify_all();
        }
        Poll::Ready(bytes.len())
    }
}

fn lines_of(printed: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(printed)
        .lines()
        .map(str::to_owned)
        .collect()
}
