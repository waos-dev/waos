//! A note: its text, whose first line is its title, and when it was made, when it last changed,
//! and when the user deleted it, which puts it in Recently Deleted.

use std::fmt;
use std::str::FromStr;
use std::time::SystemTime;

/// What a note whose text is blank is called, as Apple's Notes calls it.
const NEW_NOTE: &str = "New Note";

/// A note's own number, never given to another note of the library: the name of the file it is
/// kept in, and what its reminder names it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NoteId(pub(crate) u64);

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for NoteId {
    type Err = std::num::ParseIntError;

    fn from_str(id: &str) -> Result<NoteId, Self::Err> {
        id.parse().map(NoteId)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Note {
    pub(crate) id: NoteId,
    pub(crate) text: String,
    pub(crate) created: SystemTime,
    pub(crate) changed: SystemTime,
    /// When the user deleted it; none while it is among the notes.
    pub(crate) deleted: Option<SystemTime>,
}

impl Note {
    /// A note made `now` holding `text`.
    pub(crate) fn new(id: NoteId, text: String, now: SystemTime) -> Note {
        Note {
            id,
            text,
            created: now,
            changed: now,
            deleted: None,
        }
    }

    /// Its first line that is not blank, without the space around it; [`NEW_NOTE`] where it has
    /// none.
    pub(crate) fn title(&self) -> &str {
        self.lines().next().unwrap_or(NEW_NOTE)
    }

    /// Its line after the title, which the list shows beneath it; empty where it has none.
    pub(crate) fn preview(&self) -> &str {
        self.lines().nth(1).unwrap_or_default()
    }

    pub(crate) fn is_deleted(&self) -> bool {
        self.deleted.is_some()
    }

    /// Its lines that are not blank, without the space around them.
    fn lines(&self) -> impl Iterator<Item = &str> {
        self.text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(text: &str) -> Note {
        Note::new(NoteId(1), text.to_owned(), SystemTime::UNIX_EPOCH)
    }

    #[test]
    fn a_notes_title_is_its_first_line_and_its_preview_the_next() {
        let shopping = note("\n  Shopping  \n\nmilk, eggs\nbread");
        assert_eq!(shopping.title(), "Shopping");
        assert_eq!(shopping.preview(), "milk, eggs");
    }

    #[test]
    fn a_blank_note_is_a_new_note_with_nothing_to_preview() {
        assert_eq!(note(" \n").title(), "New Note");
        assert_eq!(note("Shopping").preview(), "");
    }

    #[test]
    fn an_id_reads_back_from_what_it_writes() {
        assert_eq!("1730900000000".parse(), Ok(NoteId(1_730_900_000_000)));
        assert_eq!(NoteId(42).to_string(), "42");
    }
}
