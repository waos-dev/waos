//! A live panel's page: a tile a row, the oldest first, titled with what it was, with when and how
//! WaOS decided it beneath, and what the row says beneath it; or why there is none; and in its
//! navigation bar, the way to start the program inspected, so the page hears it from its first
//! instruction, and to stop it.
//!
//! The invariant: a page is drawn from its `Panel` alone, so every panel looks the same.

use waft::{
    App, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, IntoWidget, Listener,
    MainAxisSize, Row, Text, WidgetRef,
};
use waft_cupertino::{CupertinoButton, CupertinoListTile, CupertinoNavigationBar};

use super::{BENEATH_LINES, TALL_TILE_PADDING, back, note, scaffold, section};
use crate::inspector::{Inspector, OWN_ID, Seen, Tab, end_program, start_inspected};
use crate::panels::{Panel, PanelRow};
use crate::words;

/// The page of `tab`'s panel of the program `seen`, with the way back to its report.
pub(super) fn panel_page(
    inspector: &Entity<Inspector>,
    seen: &Seen,
    tab: Tab,
    app: &mut App,
) -> WidgetRef {
    let live = &inspector.read(app).live;
    let mut tiles: Vec<WidgetRef> = live.refused.iter().map(|refused| note(refused)).collect();
    tiles.extend(match live.panel(tab) {
        _ if !live.heard => vec![note(words::NOT_RUNNING)],
        None => Vec::new(),
        Some(panel) => panel_tiles(panel),
    });
    let footer = (seen.program.id != OWN_ID).then_some(words::START_DETAIL);
    let sections = vec![section(Some(&seen.program.name), footer, tiles)];
    let mut bar = CupertinoNavigationBar::new()
        .leading(back(words::REPORT, inspector, |inspector, cx| {
            inspector.tab = Tab::Report;
            inspector.live.stop();
            cx.notify();
        }))
        .middle(Text::new(tab.name()));
    if seen.program.id != OWN_ID {
        bar = bar.trailing(start_buttons(inspector, seen.program.running));
    }
    scaffold(bar, sections)
}

/// Start, or Stop and Restart while it runs: the program started inspected, so the page hears it
/// from its first instruction, or ended.
fn start_buttons(inspector: &Entity<Inspector>, running: bool) -> WidgetRef {
    let button = |label: &'static str, pressed: fn(&mut App, &Entity<Inspector>)| {
        let inspector = inspector.clone();
        let press = Listener::new(move |app| pressed(app, &inspector));
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

fn panel_tiles(panel: &dyn Panel) -> Vec<WidgetRef> {
    let rows = panel.rows();
    if rows.is_empty() {
        return vec![note(panel.nothing_yet())];
    }
    rows.iter().map(|row| tile(row, panel.wraps())).collect()
}

/// One row: what it was, titled, as many lines as it takes where its panel `wraps`; when and how it
/// was decided beneath, and what it says beneath it.
fn tile(row: &PanelRow, wraps: bool) -> WidgetRef {
    let mut title = Text::new(row.title.clone());
    if wraps {
        title = title.max_lines(i32::MAX);
    }
    let mut lines = vec![Text::new(row.details.join(" · ")).into_widget()];
    lines.extend((row.beneath.iter()).map(|line| {
        Text::new(line.clone())
            .max_lines(BENEATH_LINES)
            .into_widget()
    }));
    let subtitle = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .spacing(2.0)
        .children(lines);
    CupertinoListTile::new(title)
        .subtitle(subtitle)
        .padding(TALL_TILE_PADDING)
        .into_widget()
}
