//! The home's icons as the arrow keys cross them: its rows, the first row of the apps used often
//! where the home shows one, then the rows of every app, and where a highlight goes from an icon
//! by a key, as Spotlight's results and GNOME's app grid are crossed. The home's layout and the
//! keys read the same rows, so the highlight moves over what shows.
//!
//! The invariant: a highlight a move answers is an icon of the rows, never an empty cell.

/// An icon's place among the rows: its row, counted from the top, and its place along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IconPlace {
    pub(crate) row: usize,
    pub(crate) column: usize,
}

/// A step an arrow key asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Up,
    Down,
    Left,
    Right,
}

/// The apps of the home's first row, in `columns` columns: the first `columns` of `often_used`,
/// where `installed` apps fill more than a row; none where every app is in sight in one row,
/// which a row of the same apps over it would only repeat.
pub(crate) fn first_row<T: Clone>(often_used: &[T], installed: usize, columns: usize) -> Vec<T> {
    if installed <= columns {
        return Vec::new();
    }
    often_used.iter().take(columns).cloned().collect()
}

/// The home's rows of icons: what each row holds, from the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IconRows<T> {
    rows: Vec<Vec<T>>,
}

impl<T: Clone> IconRows<T> {
    /// The rows of a home showing `first_row` over `apps`, in `columns` columns.
    pub(crate) fn new(first_row: Vec<T>, apps: &[T], columns: usize) -> IconRows<T> {
        let mut rows = Vec::new();
        if !first_row.is_empty() {
            rows.push(first_row);
        }
        rows.extend(apps.chunks(columns.max(1)).map(<[T]>::to_vec));
        IconRows { rows }
    }

    /// What is at `place`; none where the rows have no icon there.
    pub(crate) fn at(&self, place: IconPlace) -> Option<&T> {
        self.rows.get(place.row)?.get(place.column)
    }

    /// Where the highlight is after `step` from `from`; none where it leaves the icons, as Up
    /// from the top row does, going back to the search field.
    ///
    /// With nothing highlighted, Down highlights the first icon, and no other key any. Down and
    /// Up keep the column, or take the row's last icon where the row is shorter; Right and Left
    /// go on to the next row's first icon and the row before's last, and stay at the ends.
    pub(crate) fn after(&self, from: Option<IconPlace>, step: Step) -> Option<IconPlace> {
        let Some(from) = from.filter(|place| self.at(*place).is_some()) else {
            return (step == Step::Down && !self.rows.is_empty())
                .then_some(IconPlace { row: 0, column: 0 });
        };
        let last_of = |row: usize| self.rows[row].len() - 1;
        let last_row = self.rows.len() - 1;
        let place = |row: usize, column: usize| Some(IconPlace { row, column });
        match step {
            Step::Up if from.row == 0 => None,
            Step::Up => place(from.row - 1, from.column.min(last_of(from.row - 1))),
            Step::Down if from.row == last_row => Some(from),
            Step::Down => place(from.row + 1, from.column.min(last_of(from.row + 1))),
            Step::Left if from.column > 0 => place(from.row, from.column - 1),
            Step::Left if from.row > 0 => place(from.row - 1, last_of(from.row - 1)),
            Step::Left => Some(from),
            Step::Right if from.column < last_of(from.row) => place(from.row, from.column + 1),
            Step::Right if from.row < last_row => place(from.row + 1, 0),
            Step::Right => Some(from),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(row: usize, column: usize) -> Option<IconPlace> {
        Some(IconPlace { row, column })
    }

    /// Two used often over five apps in three columns: rows of 2, 3 and 2.
    fn rows() -> IconRows<&'static str> {
        IconRows::new(vec!["zed", "notes"], &["a", "b", "c", "d", "e"], 3)
    }

    #[test]
    fn the_first_row_shows_only_where_the_apps_fill_more_than_a_row() {
        assert_eq!(first_row(&["zed", "notes"], 3, 3), Vec::<&str>::new());
        assert_eq!(first_row(&["zed", "notes"], 4, 3), ["zed", "notes"]);
        assert_eq!(first_row(&["a", "b", "c", "d"], 9, 3), ["a", "b", "c"]);
    }

    #[test]
    fn down_enters_the_icons_and_up_from_the_top_row_leaves_them() {
        let rows = rows();
        assert_eq!(rows.after(None, Step::Down), at(0, 0));
        assert_eq!(rows.after(None, Step::Right), None, "the field's own key");
        assert_eq!(rows.after(at(0, 1), Step::Up), None);
        assert_eq!(rows.at(IconPlace { row: 0, column: 0 }), Some(&"zed"));
    }

    #[test]
    fn down_and_up_keep_the_column_or_take_a_shorter_rows_last_icon() {
        let rows = rows();
        assert_eq!(rows.after(at(1, 2), Step::Down), at(2, 1));
        assert_eq!(rows.after(at(1, 2), Step::Up), at(0, 1));
        assert_eq!(rows.after(at(0, 0), Step::Down), at(1, 0));
        assert_eq!(rows.after(at(2, 1), Step::Down), at(2, 1), "the last row");
    }

    #[test]
    fn right_and_left_go_on_to_the_next_row_and_stay_at_the_ends() {
        let rows = rows();
        assert_eq!(rows.after(at(0, 1), Step::Right), at(1, 0));
        assert_eq!(rows.after(at(1, 0), Step::Left), at(0, 1));
        assert_eq!(rows.after(at(2, 1), Step::Right), at(2, 1));
        assert_eq!(rows.after(at(0, 0), Step::Left), at(0, 0));
    }

    #[test]
    fn a_highlight_no_icon_is_at_counts_as_none() {
        let rows = rows();
        assert_eq!(rows.after(at(5, 0), Step::Down), at(0, 0));
        assert_eq!(
            IconRows::<&str>::new(Vec::new(), &[], 3).after(None, Step::Down),
            None
        );
    }
}
