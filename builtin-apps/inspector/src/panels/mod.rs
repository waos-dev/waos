//! The live panels of a program's page, as Chrome DevTools' tabs: each a table of what the program
//! does from when the panel shows, a row an event or a use, under named columns, the shape WinUI's
//! table, once Waft has it, takes as it is (decisions 14–16 of
//! `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`). Both kits draw a panel from
//! its columns and its rows alone.
//!
//! The invariant: a row says what its events say, a refusal naming the setting that refused it;
//! and a panel keeps at most `KEPT` rows, the oldest going first.

pub(crate) mod access;
pub(crate) mod files;
pub(crate) mod in_use;
pub(crate) mod network;
pub(crate) mod output;

use std::collections::VecDeque;

use wasdk::inspect::trace::Verdict;

/// The most rows a panel keeps.
const KEPT: usize = 2000;

/// A panel's column: its title, and its width in logical points; a width of 0 shares what is left
/// with the other columns of 0.
pub(crate) type Column = (&'static str, f64);

/// A row, as both kits draw it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PanelRow {
    /// Its cells, under the panel's columns.
    pub(crate) cells: Vec<String>,
    /// What a phone's tile is titled.
    pub(crate) title: String,
    /// What a phone's tile says under its title, on one line.
    pub(crate) details: Vec<String>,
    /// What is said beneath it, a line each, in both kits: who it was for, where a file is on the
    /// machine, why WaOS refused it and where to change that.
    pub(crate) beneath: Vec<String>,
}

/// A panel's table.
pub(crate) trait Panel {
    fn columns(&self) -> &'static [Column];

    /// Its rows, the oldest first.
    fn rows(&self) -> Vec<PanelRow>;

    /// What it says while it has no row.
    fn nothing_yet(&self) -> &'static str;

    /// Whether the text of its columns of 0 wraps, as a console's lines do, rather than being cut
    /// short.
    fn wraps(&self) -> bool {
        false
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
}

/// Rows kept, the oldest first, the oldest going first past `KEPT`.
#[derive(Debug)]
pub(crate) struct Kept<T> {
    rows: VecDeque<T>,
}

impl<T> Default for Kept<T> {
    fn default() -> Kept<T> {
        Kept {
            rows: VecDeque::new(),
        }
    }
}

impl<T> Kept<T> {
    pub(crate) fn push(&mut self, row: T) {
        if self.rows.len() == KEPT {
            self.rows.pop_front();
        }
        self.rows.push_back(row);
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.rows.iter()
    }

    pub(crate) fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> {
        self.rows.iter_mut()
    }
}
