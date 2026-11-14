//! A problem Typst found in the document, as the window says it: an error, which keeps the pages
//! from being set, or a warning; what it is, where in the text, and Typst's hints for it. It
//! crosses from the typesetting worker to the window as it is.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use typst::diag::{Severity, SourceDiagnostic};
use typst::syntax::Source;
use typst::{World, WorldExt};

use super::world::DocumentWorld;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Problem {
    pub(crate) is_error: bool,
    pub(crate) message: String,
    pub(crate) hints: Vec<String>,
    pub(crate) place: Place,
}

/// Where a problem is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Place {
    /// In the document's text: its line and column, counted from 1, and its range in UTF-16 code
    /// units, which the editor's selection is in.
    Text {
        line: usize,
        column: usize,
        utf16: Range<usize>,
    },
    /// In another file, by its name.
    File(String),
    /// Nowhere Typst names.
    Unknown,
}

impl Problem {
    pub(crate) fn of(diagnostic: &SourceDiagnostic, world: &DocumentWorld) -> Problem {
        Problem {
            is_error: diagnostic.severity == Severity::Error,
            message: diagnostic.message.to_string(),
            hints: diagnostic
                .hints
                .iter()
                .map(|hint| hint.v.to_string())
                .collect(),
            place: place_of(diagnostic, world),
        }
    }
}

fn place_of(diagnostic: &SourceDiagnostic, world: &DocumentWorld) -> Place {
    let Some(id) = diagnostic.span.id() else {
        return Place::Unknown;
    };
    if id != world.main() {
        return Place::File(id.vpath().get_without_slash().to_owned());
    }
    world
        .range(diagnostic.span)
        .and_then(|bytes| place_in_text(world.main_source(), bytes))
        .unwrap_or(Place::Unknown)
}

fn place_in_text(source: &Source, bytes: Range<usize>) -> Option<Place> {
    let lines = source.lines();
    let (line, column) = lines.byte_to_line_column(bytes.start)?;
    let utf16 = lines.byte_to_utf16(bytes.start)?..lines.byte_to_utf16(bytes.end)?;
    Some(Place::Text {
        line: line + 1,
        column: column + 1,
        utf16,
    })
}
