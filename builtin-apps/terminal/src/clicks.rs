//! Counting clicks: a press soon after the last one, on the same cell, is the second or the
//! third click of one gesture, which select a word and a line.

use std::time::Duration;

/// The longest time between two presses of one double click.
const MULTI_CLICK_TIME: Duration = Duration::from_millis(500);

/// What a press selects, by how many clicks it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClickUnit {
    Cell,
    Word,
    Line,
}

/// The last press, to count the next one from.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Clicks {
    last: Option<Click>,
}

#[derive(Clone, Copy, Debug)]
struct Click {
    at: Duration,
    cell: (u16, u16),
    count: u8,
}

impl Clicks {
    /// Counts a press on `cell` at `at`, a time on the clock every press is stamped by, and
    /// answers what it selects: a cell, a word at the second click, a line at the third, and a
    /// cell again at the fourth.
    pub(crate) fn press(&mut self, at: Duration, cell: (u16, u16)) -> ClickUnit {
        let count = match self.last {
            Some(last) if last.cell == cell && at.saturating_sub(last.at) <= MULTI_CLICK_TIME => {
                last.count % 3 + 1
            }
            _ => 1,
        };
        self.last = Some(Click { at, cell, count });
        match count {
            2 => ClickUnit::Word,
            3 => ClickUnit::Line,
            _ => ClickUnit::Cell,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(milliseconds: u64) -> Duration {
        Duration::from_millis(milliseconds)
    }

    #[test]
    fn quick_presses_on_a_cell_select_a_word_then_a_line() {
        let mut clicks = Clicks::default();
        assert_eq!(clicks.press(at(0), (1, 1)), ClickUnit::Cell);
        assert_eq!(clicks.press(at(200), (1, 1)), ClickUnit::Word);
        assert_eq!(clicks.press(at(400), (1, 1)), ClickUnit::Line);
        assert_eq!(clicks.press(at(600), (1, 1)), ClickUnit::Cell);
    }

    #[test]
    fn a_slow_press_or_one_elsewhere_starts_again() {
        let mut clicks = Clicks::default();
        clicks.press(at(0), (1, 1));
        assert_eq!(clicks.press(at(900), (1, 1)), ClickUnit::Cell);
        assert_eq!(clicks.press(at(1000), (1, 2)), ClickUnit::Cell);
    }
}
