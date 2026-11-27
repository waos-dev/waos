//! The live panels of a program's page, as Chrome DevTools' tabs: each a log of what the program
//! does from when the panel listens, a typed row an event or a use. A log turns what it hears into
//! changes of its rows, an event at a time, through [`Rows`], which the app's state backs with a
//! table's source and the tests with a `Vec`; a row says what both kits draw of it: its cells under
//! its panel's columns, its kind for the panel's filters, a phone's tile and the detail pane.
//!
//! The invariant: a row says what its events say, a refusal naming the setting that refused it;
//! and a log keeps at most `KEPT` rows, the oldest going first, so one event is one change of the
//! rows whatever the log holds.

pub(crate) mod access;
pub(crate) mod files;
pub(crate) mod in_use;
pub(crate) mod network;
pub(crate) mod output;

use wasdk::inspect::trace::Verdict;

use crate::logic::detail::{Detail, Glyph};

/// The most rows a log keeps.
pub(crate) const KEPT: usize = 2000;

/// The rows a log changes: a table's source in the app, a `Vec` in the tests.
pub(crate) trait Rows<R> {
    fn len(&self) -> usize;

    fn get(&self, index: usize) -> &R;

    fn insert(&mut self, index: usize, row: R);

    fn update(&mut self, index: usize, change: impl FnOnce(&mut R));

    fn remove(&mut self, index: usize);

    /// `row` after the others, the oldest going first past `KEPT`.
    fn append(&mut self, row: R) {
        if self.len() == KEPT {
            self.remove(0);
        }
        self.insert(self.len(), row);
    }

    /// The index of the newest row `wanted` picks.
    fn newest(&self, wanted: impl Fn(&R) -> bool) -> Option<usize> {
        (0..self.len()).rev().find(|index| wanted(self.get(*index)))
    }

    fn clear(&mut self) {
        while self.len() > 0 {
            self.remove(self.len() - 1);
        }
    }
}

impl<R> Rows<R> for Vec<R> {
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn get(&self, index: usize) -> &R {
        &self[index]
    }

    fn insert(&mut self, index: usize, row: R) {
        Vec::insert(self, index, row);
    }

    fn update(&mut self, index: usize, change: impl FnOnce(&mut R)) {
        change(&mut self[index]);
    }

    fn remove(&mut self, index: usize) {
        Vec::remove(self, index);
    }
}

/// A panel's column: what it is titled, how wide, and how its cells read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Column {
    pub(crate) title: &'static str,
    pub(crate) width: Width,
    /// Whether its cells line up on the right, as a number's do.
    pub(crate) numeric: bool,
    /// Whether it steps out of the table while the detail pane is open beside it, as DevTools'
    /// narrower columns do, since the pane says it too.
    pub(crate) steps_out: bool,
}

/// How wide a column is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Width {
    /// This many logical points.
    Points(f64),
    /// What the other columns leave.
    Rest,
}

impl Column {
    pub(crate) const fn rest(title: &'static str) -> Column {
        Column {
            title,
            width: Width::Rest,
            numeric: false,
            steps_out: false,
        }
    }

    pub(crate) const fn points(title: &'static str, points: f64) -> Column {
        Column {
            title,
            width: Width::Points(points),
            numeric: false,
            steps_out: false,
        }
    }

    pub(crate) const fn numeric(title: &'static str, points: f64) -> Column {
        Column {
            title,
            width: Width::Points(points),
            numeric: true,
            steps_out: false,
        }
    }

    /// The column, stepping out while the detail pane is open.
    pub(crate) const fn stepping_out(self) -> Column {
        Column {
            steps_out: true,
            ..self
        }
    }
}

/// A moment a row says, and how it reads: the UI says it in the user's time zone with the `App`,
/// and logic never turns it into words.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Moment {
    pub(crate) at: wasdk::system_clock::Instant,
    pub(crate) wording: Wording,
}

/// A moment's instant is its two numbers, which the generated type compares by no trait of its
/// own.
impl PartialEq for Moment {
    fn eq(&self, other: &Moment) -> bool {
        (self.at.seconds, self.at.nanoseconds) == (other.at.seconds, other.at.nanoseconds)
            && self.wording == other.wording
    }
}

impl Eq for Moment {}

/// How a moment reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wording {
    /// To the millisecond: `14:03:27.012`.
    Precise,
    /// The time of day: `14:03:27`.
    TimeOfDay,
    /// Its time today, its weekday this week, else its date's day: `14:03`, `Tue`, `5 days ago`.
    Ago,
}

/// What a cell, a field or a line says: text as it is, or a moment with the words that come
/// before it, which only the UI can say.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Said {
    Text(String),
    Moment {
        /// What comes before it: `Since `.
        before: &'static str,
        moment: Moment,
    },
}

impl Said {
    pub(crate) fn text(text: impl Into<String>) -> Said {
        Said::Text(text.into())
    }

    pub(crate) fn moment(at: wasdk::system_clock::Instant, wording: Wording) -> Said {
        Said::Moment {
            before: "",
            moment: Moment { at, wording },
        }
    }

    /// The moment, with `before` before it: `Since 14:03`.
    pub(crate) fn moment_after(
        before: &'static str,
        at: wasdk::system_clock::Instant,
        wording: Wording,
    ) -> Said {
        Said::Moment {
            before,
            moment: Moment { at, wording },
        }
    }
}

impl From<String> for Said {
    fn from(text: String) -> Said {
        Said::Text(text)
    }
}

impl From<&str> for Said {
    fn from(text: &str) -> Said {
        Said::Text(text.to_owned())
    }
}

impl From<&String> for Said {
    fn from(text: &String) -> Said {
        Said::Text(text.clone())
    }
}

/// A cell: what it says, and how it reads.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cell {
    pub(crate) text: Said,
    pub(crate) tone: Tone,
    /// Whether it is a URL, a path, an address, a time or a size, which read best monospaced.
    pub(crate) code: bool,
}

impl Cell {
    pub(crate) fn plain(text: impl Into<Said>) -> Cell {
        Cell {
            text: text.into(),
            tone: Tone::Plain,
            code: false,
        }
    }

    pub(crate) fn code(text: impl Into<Said>) -> Cell {
        Cell {
            text: text.into(),
            tone: Tone::Plain,
            code: true,
        }
    }

    pub(crate) fn toned(mut self, tone: Tone) -> Cell {
        self.tone = tone;
        self
    }

    /// What sorts it among its column's cells: the instant where it says one, else its text.
    pub(crate) fn order(&self) -> (i64, u32, &str) {
        match &self.text {
            Said::Moment { moment, .. } => (moment.at.seconds, moment.at.nanoseconds, ""),
            Said::Text(text) => (0, 0, text),
        }
    }
}

/// How a cell or a field reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    /// Said less loudly: a time, a kind.
    Quiet,
    Allowed,
    /// Refused, or failed.
    Refused,
}

/// What a phone's tile says of a row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tile {
    pub(crate) title: String,
    /// Whether the title reads as code: a URL, a path, an address, an output line.
    pub(crate) code: bool,
    /// When and how it was decided, on one line.
    pub(crate) details: Vec<Said>,
    /// What is said beneath, a line each: why WaOS refused it and where to change that.
    pub(crate) beneath: Vec<String>,
}

/// A row of a table: its cells under its table's columns, the first led by a glyph where it has
/// one.
pub(crate) trait Tabular: Clone + 'static {
    fn columns() -> &'static [Column];

    /// Its cell under the column of `index`.
    fn cell_at(&self, index: usize) -> Cell;

    /// What leads its first cell.
    fn leading(&self) -> Option<Glyph>;
}

impl<R: PanelRow> Tabular for R {
    fn columns() -> &'static [Column] {
        R::COLUMNS
    }

    fn cell_at(&self, index: usize) -> Cell {
        self.cell(index)
    }

    fn leading(&self) -> Option<Glyph> {
        Some(self.glyph())
    }
}

/// A row of a panel, as both kits draw it.
pub(crate) trait PanelRow: Clone + 'static {
    /// The kinds the panel's filters pick its rows by.
    type Kind: RowKind;

    /// The panel's columns.
    const COLUMNS: &'static [Column];

    /// What its panel says while it has no row.
    const NOTHING_YET: &'static str;

    /// Whether WaOS decides its rows, so some may be refused.
    const DECIDED: bool = true;

    /// What its rows are counted as in its status: events, or uses in use.
    const COUNTED_AS: &'static str = crate::words::EVENTS;

    fn kind(&self) -> Self::Kind;

    fn glyph(&self) -> Glyph;

    /// Its cell under the column of `index`.
    fn cell(&self, index: usize) -> Cell;

    /// Whether WaOS refused it, or it failed.
    fn refused(&self) -> bool;

    /// Whether a panel's filter of `text`, lowercase, keeps it.
    fn matches(&self, text: &str) -> bool;

    fn tile(&self) -> Tile;

    /// What the detail pane says of it.
    fn detail(&self) -> Detail;

    /// What the panel's status says of `rows` besides how many there are: what they moved.
    fn summed(rows: &[&Self]) -> Option<String> {
        let _ = rows;
        None
    }
}

/// The kinds of a panel's rows, which its filters pick by.
pub(crate) trait RowKind: Copy + Eq + std::fmt::Debug + 'static {
    const ALL: &'static [Self];

    /// Its filter's name: `Requests`, `Open files`…
    fn name(self) -> &'static str;
}

/// Which of a panel's rows show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Filter<K> {
    All,
    Kind(K),
    Refused,
}

impl<K: RowKind> Filter<K> {
    /// The filters a panel of `R`s offers: all, each kind, and the refused where WaOS decides
    /// them.
    pub(crate) fn offered<R: PanelRow<Kind = K>>() -> Vec<Filter<K>> {
        let kinds = K::ALL.iter().map(|kind| Filter::Kind(*kind));
        let refused = R::DECIDED.then_some(Filter::Refused);
        std::iter::once(Filter::All)
            .chain(kinds)
            .chain(refused)
            .collect()
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Kind(kind) => kind.name(),
            Filter::Refused => "Refused",
        }
    }

    /// Whether it keeps `row`, and a filter of `text`, lowercase, with it.
    pub(crate) fn keeps<R: PanelRow<Kind = K>>(self, row: &R, text: &str) -> bool {
        let picked = match self {
            Filter::All => true,
            Filter::Kind(kind) => row.kind() == kind,
            Filter::Refused => row.refused(),
        };
        picked && (text.is_empty() || row.matches(text))
    }
}

/// How WaOS decided an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decided {
    Allowed,
    /// Refused by the setting of this key.
    Refused(String),
    /// Failed, no setting deciding it.
    Failed,
}

impl Decided {
    pub(crate) fn of(verdict: &Verdict) -> Decided {
        match verdict {
            Verdict::Allowed => Decided::Allowed,
            Verdict::Refused(setting) => Decided::Refused(setting.clone()),
            Verdict::Failed => Decided::Failed,
        }
    }

    pub(crate) fn refused(&self) -> bool {
        !matches!(self, Decided::Allowed)
    }

    /// Its cell: `Allowed`, or `Refused · network`.
    pub(crate) fn cell(&self) -> Cell {
        match self {
            Decided::Allowed => Cell::plain(crate::words::ALLOWED).toned(Tone::Allowed),
            Decided::Refused(setting) => {
                Cell::plain(format!("{} · {setting}", crate::words::REFUSED)).toned(Tone::Refused)
            }
            Decided::Failed => Cell::plain(crate::words::FAILED).toned(Tone::Refused),
        }
    }
}

/// Whether `text`, lowercase, is found in any of `said`.
pub(crate) fn found_in(text: &str, said: &[&str]) -> bool {
    said.iter().any(|said| said.to_lowercase().contains(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_keeps_the_newest_rows_the_oldest_going_first() {
        let mut rows: Vec<usize> = Vec::new();
        for row in 0..KEPT + 2 {
            Rows::append(&mut rows, row);
        }
        assert_eq!(rows.len(), KEPT);
        assert_eq!(rows[0], 2);
        assert_eq!(
            rows.newest(|row| row % 2 == 0),
            Some(KEPT - 2),
            "2000, the newest even"
        );
    }
}
