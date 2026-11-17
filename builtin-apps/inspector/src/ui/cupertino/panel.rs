//! A live panel's page, as P1 draws it: a search field and a segmented control over the panel's
//! filters, then a tile a row, the oldest first, titled with what it was, with when and how WaOS
//! decided it beneath, and why it was refused; or why there is none; and in its navigation bar,
//! the way to start the program inspected, so the page hears it from its first instruction, and
//! to stop it.
//!
//! The invariant: a page is drawn from its panel's entity alone, so every panel looks the same,
//! and it filters through the panel, as the desktop's toolbar does.

use std::rc::Rc;

use indexmap::IndexMap;
use waft::{
    App, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, IntoWidget, Listener,
    MainAxisSize, Padding, Row, Text, TextStyle, WidgetRef,
};
use waft_cupertino::{
    CupertinoButton, CupertinoListTile, CupertinoNavigationBar, CupertinoSearchTextField,
    CupertinoSegmentedControl,
};

use super::{BENEATH_LINES, TALL_TILE_PADDING, back, note, scaffold, section};
use crate::code_font::CODE_FONT;
use crate::logic::panels::{Filter, PanelRow, Tile};
use crate::state::page::{ProgramPage, Tab, end_program, show_tab, start_inspected};
use crate::state::panel::{Heard, LivePanel, Log, set_filter};
use crate::state::programs::{OWN_ID, Seen};
use crate::words;

/// The most filters a phone's segmented control offers; a panel of more offers all and refused.
const MOST_SEGMENTS: usize = 4;

/// The page of `tab`'s panel of the program `seen`, with the way back to its report.
pub(super) fn panel_page(
    page: &Entity<ProgramPage>,
    seen: &Seen,
    tab: Tab,
    app: &mut App,
) -> WidgetRef {
    let sections = match page.read(app).panels().cloned() {
        None => vec![section(None, None, vec![note(words::NOT_RUNNING)])],
        Some(panels) => match tab {
            Tab::Report => Vec::new(),
            Tab::Network => sections_of(app, &panels.network, seen),
            Tab::Files => sections_of(app, &panels.files, seen),
            Tab::Access => sections_of(app, &panels.access, seen),
            Tab::Output => sections_of(app, &panels.output, seen),
            Tab::InUse => sections_of(app, &panels.in_use, seen),
        },
    };
    let mut bar = CupertinoNavigationBar::new()
        .leading(back(words::REPORT, page, |app, page| {
            show_tab(app, page, Tab::Report);
        }))
        .middle(Text::new(tab.name()));
    if seen.program.id != OWN_ID {
        bar = bar.trailing(start_buttons(page, seen.program.running));
    }
    scaffold(bar, sections)
}

/// The panel's filters, then its rows.
fn sections_of<L: Log>(app: &App, panel: &Entity<LivePanel<L>>, seen: &Seen) -> Vec<WidgetRef> {
    let read = panel.read(app);
    let mut tiles = Vec::new();
    if let Heard::Refused(why) = &read.heard {
        tiles.push(note(why));
    }
    let source = read.rows.handle();
    let shown: Vec<&L::Row> = (source.view(app).iter())
        .map(|index| source.item(app, *index))
        .collect();
    if read.heard == Heard::Nothing {
        tiles.push(note(words::NOT_RUNNING));
    } else if shown.is_empty() {
        tiles.push(note(<L::Row as PanelRow>::NOTHING_YET));
    }
    tiles.extend(shown.iter().map(|row| tile(&row.tile())));
    let all: Vec<&L::Row> = read.all(app).collect();
    let refused = all.iter().filter(|row| row.refused()).count();
    let mut footer = words::heard(all.len(), <L::Row as PanelRow>::COUNTED_AS, refused);
    if seen.program.id != OWN_ID {
        footer = format!("{footer}. {}", words::START_DETAIL);
    }
    vec![
        filters(panel, read.filter),
        section(Some(&seen.program.name), Some(&footer), tiles),
    ]
}

/// The search field and the segmented control over the panel's filters.
fn filters<L: Log>(
    panel: &Entity<LivePanel<L>>,
    shown: Filter<<L::Row as PanelRow>::Kind>,
) -> WidgetRef {
    let mut offered = Filter::offered::<L::Row>();
    if offered.len() > MOST_SEGMENTS {
        offered.retain(|filter| matches!(filter, Filter::All | Filter::Refused));
    }
    let typing = panel.clone();
    let search = CupertinoSearchTextField::new()
        .placeholder(words::FILTER)
        .on_changed(move |app, typed| {
            let filter = typing.read(app).filter;
            set_filter(app, &typing, filter, &typed);
        });
    let mut parts = vec![search.into_widget()];
    if offered.len() > 1 {
        let segments: IndexMap<usize, WidgetRef> = (offered.iter().enumerate())
            .map(|(index, filter)| (index, Text::new(filter.name()).into_widget()))
            .collect();
        let picking = panel.clone();
        let chosen = offered.clone();
        let mut control = CupertinoSegmentedControl::new(
            segments,
            Rc::new(move |app: &mut App, index: usize| {
                let typed = picking.read(app).text.clone();
                set_filter(app, &picking, chosen[index], &typed);
            }),
        );
        if let Some(index) = offered.iter().position(|filter| *filter == shown) {
            control = control.group_value(index);
        }
        parts.push(control.into_widget());
    }
    Padding::new(EdgeInsetsGeometry::from_ltrb(16.0, 8.0, 16.0, 0.0))
        .child(
            Column::new()
                .main_axis_size(MainAxisSize::Min)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .spacing(8.0)
                .children(parts),
        )
        .into_widget()
}

/// Start, or Stop and Restart while it runs: the program started inspected, so the page hears it
/// from its first instruction, or ended.
fn start_buttons(page: &Entity<ProgramPage>, running: bool) -> WidgetRef {
    let button = |label: &'static str, pressed: fn(&mut App, &Entity<ProgramPage>)| {
        let page = page.clone();
        let press = Listener::new(move |app| pressed(app, &page));
        CupertinoButton::new(Text::new(label).into_widget(), Some(press))
            .padding(EdgeInsetsGeometry::all(0.0))
            .into_widget()
    };
    let mut buttons = Vec::new();
    if running {
        buttons.push(button(words::STOP, end_program));
    }
    let label = if running {
        words::RESTART
    } else {
        words::START
    };
    buttons.push(button(label, start_inspected));
    Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(16.0)
        .children(buttons)
        .into_widget()
}

/// One row: what it was, titled; when and how it was decided beneath, and why it was refused.
fn tile(tile: &Tile) -> WidgetRef {
    let mut lines = vec![Text::new(tile.details.join(" · ")).into_widget()];
    lines.extend((tile.beneath.iter()).map(|line| {
        Text::new(line.clone())
            .max_lines(BENEATH_LINES)
            .into_widget()
    }));
    let subtitle = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(2.0)
        .children(lines);
    let mut title = Text::new(tile.title.clone());
    if tile.code {
        title = title.style(TextStyle::new().font_family(CODE_FONT));
    }
    CupertinoListTile::new(title)
        .subtitle(subtitle)
        .padding(TALL_TILE_PADDING)
        .into_widget()
}
