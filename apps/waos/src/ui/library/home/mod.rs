//! The library's home: the programs installed as a grid of icons, each its icon with its name on
//! one line beneath, a dot under the name while it runs and a badge on the icon while a source
//! lists a newer version; while its code is compiled, as at its install, the icon dimmed and
//! Installing… in place of its name, as iOS shows an app it installs; a tap opens it. With nothing
//! installed, one line and a button to get programs; while the user searches, as a phone's deck lets them from its bar, only the programs
//! searched for, and one line where none is. Above the grid, while any program records, who uses the microphone or the camera.
//! What is shared between the layouts is here: what each icon shows, read from the library, and
//! the grid's rows; the controls, sizes and colours are each layout's (`cupertino`, `fluent`).
//!
//! The invariant: the home shows exactly the programs installed, those searched for while the user
//! searches, in the order of their names, and changes nothing but through the library's calls.

pub(crate) mod cupertino;
pub(crate) mod fluent;

use std::sync::Arc;

use waft::{
    App, BorderRadiusGeometry, BoxDecoration, BoxShape, BuildContext, Center, Color, Column,
    CrossAxisAlignment, DecoratedBox, Expanded, IntoWidget, MainAxisSize, Opacity, Row, SizedBox,
    StatelessWidget, Text, TextAlign, TextOverflow, TextStyle, WidgetRef,
};
use waos_package::ProgramId;

use crate::state::library::Library;
use crate::ui::library::words;

/// One installed program as its icon on the home shows it.
#[derive(Clone, Debug)]
pub(crate) struct HomeEntry {
    pub(crate) id: ProgramId,
    pub(crate) name: String,
    pub(crate) icon: Option<Arc<[u8]>>,
    pub(crate) running: bool,
    pub(crate) update_offered: bool,
    /// Its code is being compiled, as at its install: it is not ready to open.
    pub(crate) installing: bool,
}

/// The programs `library` has installed, as the home shows them, in the order of their names:
/// those the user searches for, while the user searches.
pub(crate) fn home_entries(library: &Library) -> Vec<HomeEntry> {
    let mut entries: Vec<HomeEntry> = library
        .installed()
        .iter()
        .filter(|program| is_searched_for(&program.name(), library.search()))
        .map(|program| {
            let id = program.manifest.id();
            HomeEntry {
                id: id.clone(),
                name: program.name(),
                icon: library.icon(id).cloned(),
                running: library.is_running(id),
                update_offered: library.update_offered(id),
                installing: library.is_compiling(id),
            }
        })
        .collect();
    entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
    entries
}

/// Whether a program named `name` is one a user who typed `search` searches for: one whose name
/// has what was typed in it, whatever the case of either; every program, while nothing is typed.
pub(crate) fn is_searched_for(name: &str, search: &str) -> bool {
    name.to_lowercase()
        .contains(search.trim().to_lowercase().as_str())
}

/// How an icon's name and its running dot sit under it, in a layout's sizes and colours.
#[derive(Clone, Debug)]
pub(crate) struct TileLook {
    pub(crate) name_style: TextStyle,
    /// Between the icon and its name.
    pub(crate) name_gap: f64,
    pub(crate) dot_color: Color,
}

/// How large the running dot is, and how far under the name.
const DOT_SIDE: f64 = 4.0;
const DOT_GAP: f64 = 3.0;

/// How much of an icon shows while its program installs.
const INSTALLING_OPACITY: f64 = 0.5;

/// An icon with its name beneath on one line, cut with an ellipsis, and the running dot under the
/// name while the program runs; the dot's room is kept when it does not, so names line up. The
/// name takes the tile's whole width, centred in it. While the program installs, the icon is
/// dimmed and Installing… stands in place of the name.
#[derive(Clone, Debug)]
pub(crate) struct IconTile {
    pub(crate) icon: WidgetRef,
    pub(crate) name: String,
    pub(crate) running: bool,
    pub(crate) installing: bool,
    pub(crate) look: TileLook,
}

impl StatelessWidget for IconTile {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        let (icon, name) = if self.installing {
            let dimmed = Opacity::new(INSTALLING_OPACITY).child(self.icon.clone());
            (dimmed.into_widget(), words::INSTALLING.to_owned())
        } else {
            (self.icon.clone(), self.name.clone())
        };
        let name = Text::new(name)
            .style(self.look.name_style.clone())
            .max_lines(1)
            .soft_wrap(false)
            .overflow(TextOverflow::Ellipsis)
            .text_align(TextAlign::Center);
        let dot = if self.running {
            RunningDot {
                color: self.look.dot_color,
            }
            .into_widget()
        } else {
            SizedBox::new().height(DOT_SIDE).into_widget()
        };
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Center::new().child(icon).into_widget(),
                SizedBox::new().height(self.look.name_gap).into_widget(),
                name.into_widget(),
                SizedBox::new().height(DOT_GAP).into_widget(),
                Center::new().child(dot).into_widget(),
            ])
            .into_widget()
    }
}

/// The dot under a running program's name, as a Dock marks an app that runs.
#[derive(Clone, Debug)]
pub(crate) struct RunningDot {
    pub(crate) color: Color,
}

impl StatelessWidget for RunningDot {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        let decoration = BoxDecoration {
            color: Some(self.color.into()),
            shape: BoxShape::Circle,
            ..BoxDecoration::default()
        };
        SizedBox::new()
            .width(DOT_SIDE)
            .height(DOT_SIDE)
            .child(DecoratedBox::new(decoration))
            .into_widget()
    }
}

/// `tiles` in rows of `columns`, each tile in a cell of equal width, the last row's empty cells
/// kept so its tiles stay in their columns.
pub(crate) fn grid_rows(tiles: Vec<WidgetRef>, columns: usize, row_gap: f64) -> Vec<WidgetRef> {
    let columns = columns.max(1);
    let mut rows = Vec::new();
    let mut tiles = tiles.into_iter().peekable();
    while tiles.peek().is_some() {
        let cells = (0..columns).map(|_| match tiles.next() {
            Some(tile) => Expanded::new(tile).into_widget(),
            None => Expanded::new(SizedBox::shrink()).into_widget(),
        });
        if !rows.is_empty() {
            rows.push(SizedBox::new().height(row_gap).into_widget());
        }
        rows.push(
            Row::new()
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(cells.collect::<Vec<_>>())
                .into_widget(),
        );
    }
    rows
}

/// A badge's mark: a dot of `color`, `side` points across.
pub(crate) fn badge_dot(color: Color, side: f64) -> WidgetRef {
    let decoration = BoxDecoration {
        color: Some(color.into()),
        shape: BoxShape::Circle,
        ..BoxDecoration::default()
    };
    SizedBox::new()
        .width(side)
        .height(side)
        .child(DecoratedBox::new(decoration))
        .into_widget()
}

/// A rounded square of `color`, for a quiet banner's ground.
pub(crate) fn banner_ground(color: Color, radius: f64) -> BoxDecoration {
    BoxDecoration {
        color: Some(color.into()),
        border_radius: Some(BorderRadiusGeometry::circular(radius)),
        ..BoxDecoration::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_is_searched_for_by_any_part_of_its_name_whatever_the_case() {
        assert!(is_searched_for("Hardware buffers", "buff"));
        assert!(is_searched_for("Hardware buffers", "HARD"));
        assert!(is_searched_for("Gallery", " gall "));
        assert!(!is_searched_for("Gallery", "tri"));
    }

    #[test]
    fn every_program_is_shown_while_nothing_is_typed() {
        assert!(is_searched_for("Gallery", ""));
        assert!(is_searched_for("Gallery", "  "));
    }
}
