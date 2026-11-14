//! The Fluent icons the app shows, at one size: those `FluentSymbol` does not name by their code
//! points in the icon font `waft-winui` bundles, the 20-point regular ones its symbols are.

use waft_winui::{FluentSymbol, FontIcon};

/// `ic_fluent_compose_20_regular`: New Note.
const COMPOSE: char = '\u{F314}';

/// `ic_fluent_delete_20_regular`: Delete, and Recently Deleted.
const DELETE: char = '\u{F34C}';

/// `ic_fluent_document_20_regular`: a file open, in the list.
const DOCUMENT: char = '\u{F378}';

/// `ic_fluent_note_20_regular`: a note in the list.
const NOTE: char = '\u{F56B}';

/// The size icons are shown at in the list and above the note, in logical points.
const ICON_SIZE: f64 = 16.0;

/// Close File.
pub(crate) fn close() -> FontIcon {
    FontIcon::symbol(FluentSymbol::Dismiss).font_size(ICON_SIZE)
}

pub(crate) fn compose() -> FontIcon {
    icon(COMPOSE)
}

pub(crate) fn delete() -> FontIcon {
    icon(DELETE)
}

pub(crate) fn document() -> FontIcon {
    icon(DOCUMENT)
}

pub(crate) fn note() -> FontIcon {
    icon(NOTE)
}

fn icon(glyph: char) -> FontIcon {
    FontIcon::new(glyph).font_size(ICON_SIZE)
}
