//! What a session's screen is drawn from at one moment: each row's picture, the selection and a
//! block cursor already in it, and the cursor's shape where it is drawn over the rows.

use crate::palette::Palette;
use crate::row_picture::{Highlights, RowPicture, row_picture};
use crate::session::Session;

/// How the cursor is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CursorShape {
    /// The whole cell, its character shown in the background's colour: part of its row's picture.
    Block,
    /// The cell's outline, while the window does not have the keyboard.
    Outline,
    /// A line at the cell's left edge.
    Bar,
    /// A line under the cell.
    Underline,
}

/// The cursor, at a row and a column of the rows shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cursor {
    pub(crate) row: u16,
    pub(crate) column: u16,
    pub(crate) shape: CursorShape,
}

/// The rows shown, top first, and the cursor where it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FramePicture {
    pub(crate) rows: Vec<RowPicture>,
    pub(crate) cursor: Option<Cursor>,
}

/// The picture of what `session` shows now, in `palette`, the cursor drawn as that of a window
/// that has the keyboard or not, as `focused` says. A session whose shell does not run shows no
/// cursor.
pub(crate) fn frame_picture(session: &Session, palette: &Palette, focused: bool) -> FramePicture {
    let screen = &session.screen;
    let size = screen.size();
    let cursor = screen
        .cursor()
        .filter(|_| session.is_running())
        .map(|(row, column)| Cursor {
            row,
            column,
            shape: cursor_shape(screen.cursor_style(), focused),
        });
    let rows = (0..size.rows)
        .map(|row| {
            let line = screen.first_shown_line() + u64::from(row);
            let highlights = Highlights {
                selected: session
                    .selection
                    .as_ref()
                    .and_then(|selection| selection.columns_of(line, size.columns)),
                block_cursor: cursor
                    .filter(|cursor| cursor.row == row && cursor.shape == CursorShape::Block)
                    .map(|cursor| cursor.column),
            };
            row_picture(screen, row, palette, &highlights)
        })
        .collect();
    FramePicture { rows, cursor }
}

/// The shape a command asked the cursor to take, a block unless it asked; an outline while the
/// window does not have the keyboard.
fn cursor_shape(style: vt100::CursorStyle, focused: bool) -> CursorShape {
    use vt100::CursorStyle as Style;
    if !focused {
        return CursorShape::Outline;
    }
    match style {
        Style::Default | Style::BlinkBlock | Style::SteadyBlock => CursorShape::Block,
        Style::BlinkUnderline | Style::SteadyUnderline => CursorShape::Underline,
        Style::BlinkVerticalBar | Style::SteadyVerticalBar => CursorShape::Bar,
    }
}
