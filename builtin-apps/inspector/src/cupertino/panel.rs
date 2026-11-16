//! A live panel's page: a tile a row, the oldest first, titled with what it was, with when and how
//! WaOS decided it beneath, and what the row says beneath it; or why there is none.
//!
//! The invariant: a page is drawn from its `Panel` alone, so every panel looks the same.

use waft::{App, Column, CrossAxisAlignment, Entity, IntoWidget, MainAxisSize, Text, WidgetRef};
use waft_cupertino::{CupertinoListTile, CupertinoNavigationBar};

use super::{BENEATH_LINES, TALL_TILE_PADDING, back, note, scaffold, section};
use crate::inspector::{Inspector, Seen, Tab};
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
    let tiles = match live.panel(tab) {
        _ if !live.heard => vec![note(words::NOT_RUNNING)],
        None => Vec::new(),
        Some(panel) => tiles(panel),
    };
    let sections = vec![section(Some(&seen.program.name), None, tiles)];
    let bar = CupertinoNavigationBar::new()
        .leading(back(words::REPORT, inspector, |inspector, cx| {
            inspector.tab = Tab::Report;
            inspector.live.stop();
            cx.notify();
        }))
        .middle(Text::new(tab.name()));
    scaffold(bar, sections)
}

fn tiles(panel: &dyn Panel) -> Vec<WidgetRef> {
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
