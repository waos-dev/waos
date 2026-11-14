//! The text a user selected: from the cell the press was on to the cell the pointer is over, as
//! text reads, whichever of the two comes first.

use std::ops::Range;

use crate::screen::BufferPoint;

/// A run of selected cells, from where the selection was started to where it reaches now, both
/// included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    anchor: CellRange,
    head: CellRange,
}

/// The cells one end of a selection covers: a cell, or the word or the line a double or a triple
/// click took, its first and last cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CellRange {
    pub(crate) first: BufferPoint,
    pub(crate) last: BufferPoint,
}

impl Selection {
    /// A selection of `cells` alone, which a drag then extends from.
    pub(crate) fn of(cells: CellRange) -> Selection {
        Selection {
            anchor: cells,
            head: cells,
        }
    }

    /// This selection reaching to `cells`, from where it was started.
    pub(crate) fn extended_to(self, cells: CellRange) -> Selection {
        Selection {
            anchor: self.anchor,
            head: cells,
        }
    }

    /// The first selected cell.
    pub(crate) fn start(&self) -> BufferPoint {
        self.anchor.first.min(self.head.first)
    }

    /// The last selected cell.
    pub(crate) fn end(&self) -> BufferPoint {
        self.anchor.last.max(self.head.last)
    }

    /// The columns of `line` the selection covers, of the `columns` a line has; none where it
    /// covers none.
    pub(crate) fn columns_of(&self, line: u64, columns: u16) -> Option<Range<u16>> {
        let (start, end) = (self.start(), self.end());
        if line < start.line || line > end.line {
            return None;
        }
        let first = if line == start.line { start.column } else { 0 };
        let past_last = if line == end.line {
            end.column.saturating_add(1).min(columns)
        } else {
            columns
        };
        (first < past_last).then_some(first..past_last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(line: u64, column: u16) -> BufferPoint {
        BufferPoint { line, column }
    }

    fn cell(point: BufferPoint) -> CellRange {
        CellRange {
            first: point,
            last: point,
        }
    }

    fn dragged(from: BufferPoint, to: BufferPoint) -> Selection {
        Selection::of(cell(from)).extended_to(cell(to))
    }

    #[test]
    fn a_drag_backwards_selects_the_same_cells() {
        let forwards = dragged(point(1, 2), point(3, 4));
        let backwards = dragged(point(3, 4), point(1, 2));
        assert_eq!(forwards.start(), backwards.start());
        assert_eq!(forwards.end(), backwards.end());
    }

    #[test]
    fn each_line_is_covered_from_the_start_to_the_end() {
        let selection = dragged(point(1, 2), point(3, 4));
        assert_eq!(selection.columns_of(0, 10), None);
        assert_eq!(selection.columns_of(1, 10), Some(2..10));
        assert_eq!(selection.columns_of(2, 10), Some(0..10));
        assert_eq!(selection.columns_of(3, 10), Some(0..5));
        assert_eq!(selection.columns_of(4, 10), None);
    }

    #[test]
    fn a_word_stays_selected_while_the_drag_goes_back_past_it() {
        let word = CellRange {
            first: point(2, 4),
            last: point(2, 8),
        };
        let selection = Selection::of(word).extended_to(cell(point(2, 1)));
        assert_eq!(selection.start(), point(2, 1));
        assert_eq!(selection.end(), point(2, 8));
    }
}
