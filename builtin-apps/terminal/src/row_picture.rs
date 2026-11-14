//! What one row of the screen is drawn from: the cells' fills, and their text gathered into runs
//! that each take one style, so a row is a few pieces of text to lay out and not a cell at a
//! time.
//!
//! The invariant a later editor will break: a run holds only characters the terminal's own font
//! draws one cell wide, a blank cell inside it as a space, so the run's characters fall on their
//! cells when it is laid out at the cell's width. A character of another width, or one another
//! font may draw, is a run of its own, placed at its cell.

use std::ops::Range;

use crate::palette::{Palette, Rgb, dimmed};
use crate::screen::Screen;

/// How a run of text is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RunStyle {
    pub(crate) color: Rgb,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
}

/// Text drawn in one style from the cell at `column` on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextRun {
    pub(crate) column: u16,
    pub(crate) text: String,
    pub(crate) style: RunStyle,
}

/// Cells filled with a colour other than the terminal's background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fill {
    pub(crate) column: u16,
    pub(crate) cells: u16,
    pub(crate) color: Rgb,
}

/// What one row is drawn from, its fills beneath its text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RowPicture {
    pub(crate) fills: Vec<Fill>,
    pub(crate) runs: Vec<TextRun>,
}

/// How a row's cells are singled out as it is drawn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Highlights {
    /// The selected columns, filled with the selection's colour.
    pub(crate) selected: Option<Range<u16>>,
    /// The column a block cursor covers, drawn in the cursor's colour with its character in the
    /// background's.
    pub(crate) block_cursor: Option<u16>,
}

/// The picture of the row shown at `row`.
pub(crate) fn row_picture(
    screen: &Screen,
    row: u16,
    palette: &Palette,
    highlights: &Highlights,
) -> RowPicture {
    let mut picture = RowPicture::default();
    for column in 0..screen.size().columns {
        let Some(cell) = screen.cell(row, column) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let colors = cell_colors(cell, column, palette, highlights);
        let cells = if cell.is_wide() { 2 } else { 1 };
        if let Some(fill) = colors.fill {
            add_fill(&mut picture.fills, column, cells, fill);
        }
        let style = RunStyle {
            color: colors.text,
            bold: cell.bold(),
            italic: cell.italic(),
            underline: cell.underline(),
        };
        add_text(&mut picture.runs, column, cell, style);
    }
    trim_trailing_blanks(&mut picture.runs);
    picture
}

/// The colours a cell is drawn in.
struct CellColors {
    text: Rgb,
    fill: Option<Rgb>,
}

/// The cell's colours as the command set them, swapped where it asked for inverse, then as the
/// selection and the cursor show them.
fn cell_colors(
    cell: &vt100::Cell,
    column: u16,
    palette: &Palette,
    highlights: &Highlights,
) -> CellColors {
    let mut text = palette.text(cell.fgcolor(), cell.bold());
    let mut fill = palette.fill(cell.bgcolor());
    if cell.inverse() {
        let behind = fill.unwrap_or(palette.background);
        fill = Some(text);
        text = behind;
    }
    if cell.dim() {
        text = dimmed(text, fill.unwrap_or(palette.background));
    }
    if highlights
        .selected
        .as_ref()
        .is_some_and(|selected| selected.contains(&column))
    {
        fill = Some(palette.selection);
    }
    if highlights.block_cursor == Some(column) {
        fill = Some(palette.cursor);
        text = palette.background;
    }
    CellColors { text, fill }
}

/// Adds `cells` cells of `color` from `column`, to the fill before them where it is the same
/// colour and ends there.
fn add_fill(fills: &mut Vec<Fill>, column: u16, cells: u16, color: Rgb) {
    if let Some(last) = fills.last_mut()
        && last.color == color
        && last.column + last.cells == column
    {
        last.cells += cells;
        return;
    }
    fills.push(Fill {
        column,
        cells,
        color,
    });
}

/// Adds the cell's text at `column`: to the run before it where the run reaches the cell in the
/// same style and both are in the terminal's own font, and as a run of its own otherwise.
fn add_text(runs: &mut Vec<TextRun>, column: u16, cell: &vt100::Cell, style: RunStyle) {
    let contents = cell.contents();
    let own_font = !cell.is_wide() && is_in_own_font(contents);
    if let Some(last) = runs.last_mut()
        && own_font
        && last.style == style
        && is_in_own_font(&last.text)
        && last.column + last.text.chars().count() as u16 == column
    {
        last.text.push_str(shown(contents));
        return;
    }
    if contents.is_empty() {
        return;
    }
    runs.push(TextRun {
        column,
        text: contents.to_owned(),
        style,
    });
}

/// A blank cell inside a run is a space.
fn shown(contents: &str) -> &str {
    if contents.is_empty() { " " } else { contents }
}

/// Whether every character of `text` is one a monospace font draws itself, one cell wide: ASCII,
/// Latin-1, and the box-drawing and block characters full-screen programs draw their frames
/// with. A blank cell's empty text is.
fn is_in_own_font(text: &str) -> bool {
    text.chars().all(
        |character| matches!(character, ' '..='~' | '\u{a0}'..='\u{ff}' | '\u{2500}'..='\u{259f}'),
    )
}

/// Drops the spaces a run ends with, which blank cells after its text added, and a run of
/// nothing else; an underlined run keeps them, since its underline shows.
fn trim_trailing_blanks(runs: &mut Vec<TextRun>) {
    for run in runs.iter_mut().filter(|run| !run.style.underline) {
        let kept = run.text.trim_end_matches(' ').len();
        run.text.truncate(kept);
    }
    runs.retain(|run| !run.text.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::GridSize;

    fn picture_of(output: &[u8], highlights: &Highlights) -> RowPicture {
        let mut screen = Screen::new(GridSize::new(20, 2));
        screen.feed(output);
        row_picture(&screen, 0, &Palette::DARK, highlights)
    }

    fn texts(picture: &RowPicture) -> Vec<(u16, &str)> {
        picture
            .runs
            .iter()
            .map(|run| (run.column, run.text.as_str()))
            .collect()
    }

    #[test]
    fn plain_text_is_one_run_with_no_fill() {
        let picture = picture_of(b"ls -la", &Highlights::default());
        assert_eq!(texts(&picture), [(0, "ls -la")]);
        assert_eq!(picture.fills, []);
    }

    #[test]
    fn a_change_of_colour_starts_a_run() {
        let picture = picture_of(b"a \x1b[31mred\x1b[0m b", &Highlights::default());
        assert_eq!(texts(&picture), [(0, "a"), (2, "red"), (5, " b")]);
        assert_ne!(picture.runs[0].style.color, picture.runs[1].style.color);
    }

    #[test]
    fn a_gap_the_cursor_jumped_is_spaces_inside_the_run() {
        let picture = picture_of(b"a\x1b[1;5Hb", &Highlights::default());
        assert_eq!(texts(&picture), [(0, "a   b")]);
    }

    #[test]
    fn a_wide_character_is_a_run_of_its_own_at_its_cell() {
        let picture = picture_of("ab\u{4f60}cd".as_bytes(), &Highlights::default());
        assert_eq!(texts(&picture), [(0, "ab"), (2, "\u{4f60}"), (4, "cd")]);
    }

    #[test]
    fn a_background_is_one_fill_across_its_cells() {
        let picture = picture_of(b"\x1b[44mab\x1b[0mc", &Highlights::default());
        assert_eq!(picture.fills.len(), 1);
        assert_eq!((picture.fills[0].column, picture.fills[0].cells), (0, 2));
    }

    #[test]
    fn inverse_swaps_the_text_and_what_is_behind_it() {
        let picture = picture_of(b"\x1b[7mx", &Highlights::default());
        assert_eq!(picture.fills[0].color, Palette::DARK.foreground);
        assert_eq!(picture.runs[0].style.color, Palette::DARK.background);
    }

    #[test]
    fn selected_cells_are_filled_even_where_they_are_blank() {
        let highlights = Highlights {
            selected: Some(1..4),
            block_cursor: None,
        };
        let picture = picture_of(b"ab", &highlights);
        assert_eq!(picture.fills.len(), 1);
        assert_eq!((picture.fills[0].column, picture.fills[0].cells), (1, 3));
        assert_eq!(picture.fills[0].color, Palette::DARK.selection);
    }

    #[test]
    fn a_block_cursor_shows_its_character_in_the_background_colour() {
        let highlights = Highlights {
            selected: None,
            block_cursor: Some(1),
        };
        let picture = picture_of(b"abc", &highlights);
        assert_eq!(texts(&picture), [(0, "a"), (1, "b"), (2, "c")]);
        assert_eq!(picture.runs[1].style.color, Palette::DARK.background);
        assert_eq!(picture.fills[0].color, Palette::DARK.cursor);
    }
}
