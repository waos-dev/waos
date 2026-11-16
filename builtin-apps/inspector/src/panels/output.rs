//! What a program writes to its standard output and errors, as a console shows it: a row per line,
//! when it began, which stream and which instance wrote it, and the line, wrapped rather than cut.
//!
//! The invariant: a line is whole however the program's writes cut it, a character split between
//! two writes included; a line not ended yet shows as far as it is written and grows as more comes.

use std::collections::HashMap;

use wasdk::inspect::trace::{OutputChunk, OutputStream};

use super::{Column, Kept, Panel, PanelRow};
use crate::{clock, words};

const COLUMNS: [Column; 3] = [("Time", 76.0), ("Stream", 96.0), ("Line", 0.0)];

/// A stream of an instance: the instance, and whether it is the errors.
type Writer = (u32, bool);

/// The lines heard so far.
#[derive(Debug, Default)]
pub(crate) struct OutputLog {
    rows: Kept<OutputLine>,
    /// The bytes of a character each writer began and has not ended yet.
    unfinished: HashMap<Writer, Vec<u8>>,
}

impl OutputLog {
    pub(crate) fn add(&mut self, chunk: &OutputChunk) {
        let writer = (chunk.instance, chunk.channel == OutputStream::Stderr);
        let text = self.text_of(writer, &chunk.bytes);
        let mut pieces = text.split('\n');
        if let Some(first) = pieces.next() {
            self.continue_line(writer, chunk.at.seconds, first);
        }
        for piece in pieces {
            self.end_line(writer);
            self.continue_line(writer, chunk.at.seconds, piece);
        }
    }

    /// `bytes` as text, after what `writer` left unfinished, keeping a character it leaves
    /// unfinished for its next write.
    fn text_of(&mut self, writer: Writer, bytes: &[u8]) -> String {
        let mut whole = self.unfinished.remove(&writer).unwrap_or_default();
        whole.extend_from_slice(bytes);
        let finished = match std::str::from_utf8(&whole) {
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            _ => whole.len(),
        };
        let rest = whole.split_off(finished);
        if !rest.is_empty() {
            self.unfinished.insert(writer, rest);
        }
        String::from_utf8_lossy(&whole).into_owned()
    }

    /// `text` at the end of `writer`'s line not ended yet, or as a new line.
    fn continue_line(&mut self, writer: Writer, at: i64, text: &str) {
        let text = text.trim_end_matches('\r');
        let open = self
            .rows
            .iter_mut()
            .rev()
            .find(|line| line.writer == writer && line.open);
        match open {
            Some(line) => line.text.push_str(text),
            None if text.is_empty() => {}
            None => self.rows.push(OutputLine {
                at,
                writer,
                text: text.to_owned(),
                open: true,
            }),
        }
    }

    fn end_line(&mut self, writer: Writer) {
        let open = self
            .rows
            .iter_mut()
            .rev()
            .find(|line| line.writer == writer && line.open);
        match open {
            Some(line) => line.open = false,
            // An empty line, which `continue_line` keeps no row for while it is open.
            None => self.rows.push(OutputLine {
                at: 0,
                writer,
                text: String::new(),
                open: false,
            }),
        }
    }
}

impl Panel for OutputLog {
    fn columns(&self) -> &'static [Column] {
        &COLUMNS
    }

    fn rows(&self) -> Vec<PanelRow> {
        self.rows.iter().map(OutputLine::drawn).collect()
    }

    fn nothing_yet(&self) -> &'static str {
        words::NO_OUTPUT_YET
    }

    fn wraps(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
struct OutputLine {
    /// When it began; 0 for an empty line, which says no time.
    at: i64,
    writer: Writer,
    text: String,
    /// Whether it is not ended yet.
    open: bool,
}

impl OutputLine {
    fn drawn(&self) -> PanelRow {
        let (instance, errors) = self.writer;
        let time = if self.at == 0 {
            String::new()
        } else {
            clock::time_of_day(self.at)
        };
        let stream = words::stream(instance, errors);
        PanelRow {
            cells: vec![time.clone(), stream.clone(), self.text.clone()],
            title: self.text.clone(),
            details: [time, stream]
                .into_iter()
                .filter(|said| !said.is_empty())
                .collect(),
            beneath: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasdk::system_clock::Instant;

    fn chunk(bytes: &[u8]) -> OutputChunk {
        OutputChunk {
            at: Instant {
                seconds: 1,
                nanoseconds: 0,
            },
            instance: 0,
            dropped_before: 0,
            channel: OutputStream::Stdout,
            bytes: bytes.to_vec(),
        }
    }

    fn lines(log: &OutputLog) -> Vec<String> {
        log.rows.iter().map(|line| line.text.clone()).collect()
    }

    #[test]
    fn a_line_cut_between_writes_is_one_line() {
        let mut log = OutputLog::default();
        log.add(&chunk(b"hel"));
        log.add(&chunk(b"lo\nworld\n"));
        assert_eq!(lines(&log), ["hello", "world"]);
    }

    #[test]
    fn a_character_cut_between_writes_is_kept_whole() {
        let mut log = OutputLog::default();
        let bytes = "é\n".as_bytes();
        log.add(&chunk(&bytes[..1]));
        log.add(&chunk(&bytes[1..]));
        assert_eq!(lines(&log), ["é"]);
    }

    #[test]
    fn an_empty_line_is_a_row() {
        let mut log = OutputLog::default();
        log.add(&chunk(b"a\n\nb\n"));
        assert_eq!(lines(&log), ["a", "", "b"]);
    }
}
