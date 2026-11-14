//! The size of a cell: every character of the grid takes one, so the screen's size in cells
//! follows from the window's, and a point in the window names a cell.

use crate::screen::GridSize;

/// A cell's size in logical pixels, each a whole number of physical ones so that neighbouring
/// cells' fills meet without a seam.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CellMetrics {
    pub(crate) width: f64,
    pub(crate) height: f64,
    /// What is added after each character so that the font's own advance comes to `width`.
    pub(crate) letter_spacing: f64,
    /// How far below a cell's top its text's baseline lies.
    pub(crate) baseline: f64,
}

impl CellMetrics {
    /// The cells of a font whose characters advance by `advance` on lines `line_height` tall,
    /// their baseline `baseline` below the line's top, on a screen of `pixel_ratio` physical
    /// pixels to a logical one.
    pub(crate) fn of_font(
        advance: f64,
        line_height: f64,
        baseline: f64,
        pixel_ratio: f64,
    ) -> CellMetrics {
        let width = ((advance * pixel_ratio).round() / pixel_ratio).max(1.0 / pixel_ratio);
        let height = ((line_height * pixel_ratio).ceil() / pixel_ratio).max(1.0 / pixel_ratio);
        CellMetrics {
            width,
            height,
            letter_spacing: width - advance,
            baseline,
        }
    }

    /// The screen that fits in `width` by `height` logical pixels.
    pub(crate) fn grid_in(&self, width: f64, height: f64) -> GridSize {
        GridSize::new(
            whole_cells(width / self.width),
            whole_cells(height / self.height),
        )
    }

    /// The row and the column of the cell at `x` and `y`, in a grid of `size`: the nearest cell
    /// for a point outside it.
    pub(crate) fn cell_at(&self, x: f64, y: f64, size: GridSize) -> (u16, u16) {
        let row = whole_cells(y / self.height).min(size.rows - 1);
        let column = whole_cells(x / self.width).min(size.columns - 1);
        (row, column)
    }
}

/// `cells` as a count of whole cells, none for a negative or an unbounded one.
fn whole_cells(cells: f64) -> u16 {
    if cells.is_finite() {
        cells.floor().clamp(0.0, f64::from(u16::MAX)) as u16
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_is_a_whole_number_of_physical_pixels() {
        let metrics = CellMetrics::of_font(7.83, 15.2, 12.0, 2.0);
        assert_eq!(metrics.width, 8.0);
        assert_eq!(metrics.height, 15.5);
        assert!((metrics.letter_spacing - 0.17).abs() < 1e-9);
    }

    #[test]
    fn the_grid_is_the_whole_cells_that_fit() {
        let metrics = CellMetrics::of_font(8.0, 16.0, 12.0, 1.0);
        assert_eq!(metrics.grid_in(645.0, 390.0), GridSize::new(80, 24));
        assert_eq!(metrics.grid_in(3.0, 3.0), GridSize::new(1, 1));
    }

    #[test]
    fn a_point_names_its_cell_and_the_nearest_one_outside_the_grid() {
        let metrics = CellMetrics::of_font(8.0, 16.0, 12.0, 1.0);
        let size = GridSize::new(80, 24);
        assert_eq!(metrics.cell_at(17.0, 33.0, size), (2, 2));
        assert_eq!(metrics.cell_at(-5.0, 9999.0, size), (23, 0));
    }
}
