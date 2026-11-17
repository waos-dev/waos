//! What a program writes to its standard output and errors, as a console shows it: a row per line,
//! when it began, which stream and which instance wrote it, and the line; the detail pane holds a
//! line too long for its column whole.
//!
//! The invariant: a line is whole however the program's writes cut it, a character split between
//! two writes included; a line not ended yet shows as far as it is written and grows as more comes.

use std::collections::HashMap;

use wasdk::inspect::trace::{OutputChunk, OutputStream};
use wasdk::system_clock::Instant;

use super::{Cell, Column, PanelRow, RowKind, Rows, Tile, Tone, found_in};
use crate::clock;
use crate::logic::detail::{Detail, Field, Glyph, Section};
use crate::words;

/// A stream of an instance: the instance, and whether it is the errors.
type Writer = (u32, bool);

/// What the panel hears, line by line.
#[derive(Debug, Default)]
pub(crate) struct OutputLog {
    /// The bytes of a character each writer began and has not ended yet.
    unfinished: HashMap<Writer, Vec<u8>>,
}

impl OutputLog {
    pub(crate) fn add(&mut self, rows: &mut impl Rows<OutputLine>, chunk: &OutputChunk) {
        let writer = (chunk.instance, chunk.channel == OutputStream::Stderr);
        let text = self.text_of(writer, &chunk.bytes);
        let mut pieces = text.split('\n');
        if let Some(first) = pieces.next() {
            continue_line(rows, writer, chunk.at, first);
        }
        for piece in pieces {
            end_line(rows, writer);
            continue_line(rows, writer, chunk.at, piece);
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
}

/// `text` at the end of `writer`'s line not ended yet, or as a new line.
fn continue_line(rows: &mut impl Rows<OutputLine>, writer: Writer, at: Instant, text: &str) {
    let text = text.trim_end_matches('\r');
    match open_line(rows, writer) {
        Some(index) if !text.is_empty() => rows.update(index, |line| line.text.push_str(text)),
        Some(_) => {}
        None if text.is_empty() => {}
        None => rows.append(OutputLine {
            at: Some(at),
            writer,
            text: text.to_owned(),
            open: true,
        }),
    }
}

fn end_line(rows: &mut impl Rows<OutputLine>, writer: Writer) {
    match open_line(rows, writer) {
        Some(index) => rows.update(index, |line| line.open = false),
        // An empty line, which `continue_line` keeps no row for while it is open.
        None => rows.append(OutputLine {
            at: None,
            writer,
            text: String::new(),
            open: false,
        }),
    }
}

/// The index of `writer`'s line not ended yet.
fn open_line(rows: &impl Rows<OutputLine>, writer: Writer) -> Option<usize> {
    rows.newest(|line| line.writer == writer && line.open)
}

/// Which stream wrote a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputKind {
    Output,
    Errors,
}

impl RowKind for OutputKind {
    const ALL: &'static [OutputKind] = &[OutputKind::Output, OutputKind::Errors];

    fn name(self) -> &'static str {
        match self {
            OutputKind::Output => "Output",
            OutputKind::Errors => "Errors",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OutputLine {
    /// When it began; none for an empty line, which says no time.
    at: Option<Instant>,
    writer: Writer,
    text: String,
    /// Whether it is not ended yet.
    open: bool,
}

impl OutputLine {
    fn time(&self) -> String {
        self.at.map(clock::precise).unwrap_or_default()
    }

    fn stream(&self) -> String {
        let (instance, errors) = self.writer;
        words::stream(instance, errors)
    }
}

impl PanelRow for OutputLine {
    type Kind = OutputKind;

    const COLUMNS: &'static [Column] = &[
        Column::numeric("Time", 100.0),
        Column::points("Stream", 120.0).stepping_out(),
        Column::rest("Line"),
    ];

    const NOTHING_YET: &'static str = words::NO_OUTPUT_YET;

    const DECIDED: bool = false;

    fn kind(&self) -> OutputKind {
        if self.writer.1 {
            OutputKind::Errors
        } else {
            OutputKind::Output
        }
    }

    fn glyph(&self) -> Glyph {
        Glyph::Output
    }

    fn cell(&self, index: usize) -> Cell {
        match index {
            0 => Cell::code(self.time()).toned(Tone::Quiet),
            1 => Cell::plain(self.stream()).toned(Tone::Quiet),
            _ if self.writer.1 => Cell::code(self.text.clone()).toned(Tone::Refused),
            _ => Cell::code(self.text.clone()),
        }
    }

    fn refused(&self) -> bool {
        false
    }

    fn matches(&self, text: &str) -> bool {
        found_in(text, &[&self.text])
    }

    fn tile(&self) -> Tile {
        Tile {
            code: true,
            title: self.text.clone(),
            details: [self.time(), self.stream()]
                .into_iter()
                .filter(|said| !said.is_empty())
                .collect(),
            beneath: Vec::new(),
        }
    }

    fn detail(&self) -> Detail {
        let mut fields = vec![Field::plain(words::STREAM, self.stream())];
        if self.at.is_some() {
            fields.push(Field::code(words::AT, self.time()));
        }
        Detail {
            glyph: Glyph::Output,
            title: words::LINE.to_owned(),
            subtitle: self.text.clone(),
            refusal: None,
            sections: vec![Section {
                title: words::EVENT,
                fields,
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn lines(rows: &[OutputLine]) -> Vec<String> {
        rows.iter().map(|line| line.text.clone()).collect()
    }

    #[test]
    fn a_line_cut_between_writes_is_one_line() {
        let (mut log, mut rows) = (OutputLog::default(), Vec::new());
        log.add(&mut rows, &chunk(b"hel"));
        log.add(&mut rows, &chunk(b"lo\nworld\n"));
        assert_eq!(lines(&rows), ["hello", "world"]);
    }

    #[test]
    fn a_character_cut_between_writes_is_kept_whole() {
        let (mut log, mut rows) = (OutputLog::default(), Vec::new());
        let bytes = "é\n".as_bytes();
        log.add(&mut rows, &chunk(&bytes[..1]));
        log.add(&mut rows, &chunk(&bytes[1..]));
        assert_eq!(lines(&rows), ["é"]);
    }

    #[test]
    fn an_empty_line_is_a_row() {
        let (mut log, mut rows) = (OutputLog::default(), Vec::new());
        log.add(&mut rows, &chunk(b"a\n\nb\n"));
        assert_eq!(lines(&rows), ["a", "", "b"]);
    }
}
