//! The report page: what the program did over the last seven days, as iOS's App Privacy Report
//! shows an app's: each kind of access with a strip of the days it was had on, the domains
//! contacted ranked with a bar each, the questions put; and its live panels, a tile each.
//!
//! The invariant: the page draws what `Report` says and nothing else, so both kits say the same.

use std::rc::Rc;

use waft::{
    App, AsyncCallback, BorderRadiusGeometry, BoxDecoration, Builder, Column, CrossAxisAlignment,
    DecoratedBox, Entity, IntoWidget, LayoutBuilder, MainAxisSize, Row, SizedBox, Stack, Task,
    Text, WidgetRef,
};
use waft_cupertino::{
    CupertinoColors, CupertinoListTile, CupertinoListTileChevron, CupertinoNavigationBar,
};

use super::{ICON_SIDE, TALL_TILE_PADDING, back, note, resolved, scaffold, section};
use crate::clock;
use crate::inspector::{Inspector, Seen, Tab, show_tab};
use crate::program_icon::ProgramIcon;
use crate::report::{self, Report};
use crate::words;

/// A day's cell in an access's strip.
const STRIP_CELL: f64 = 8.0;

/// The program's report, as iOS's App Privacy Report shows an app's: each kind of access with the
/// days it was had on, the domains contacted ranked, the questions put, and its live panels.
pub(super) fn report_page(inspector: &Entity<Inspector>, seen: &Seen, app: &mut App) -> WidgetRef {
    let icon = ProgramIcon {
        image: inspector.read(app).icon(&seen.program.id),
        side: ICON_SIDE,
    };
    let header = CupertinoListTile::new(Text::new(seen.program.name.clone()))
        .subtitle(Text::new(words::detail(seen)))
        .leading(icon)
        .leading_size(ICON_SIDE)
        .into_widget();
    let report = Report::of(&seen.days, clock::today());
    let mut sections = vec![section(None, None, vec![header])];
    if report.is_empty() {
        sections.push(section(None, None, vec![note(words::USED_NOTHING)]));
    }
    if !report.access.is_empty() {
        let tiles = (report.access.iter())
            .map(|access| access_tile(inspector, access))
            .collect();
        sections.push(section(
            Some(words::ACCESS),
            Some(words::ACCESS_DETAIL),
            tiles,
        ));
    }
    let domains = if report.domains.is_empty() {
        vec![note(words::NO_DOMAINS)]
    } else {
        let mut tiles: Vec<WidgetRef> = report.domains.iter().map(domain_tile).collect();
        if report.local_addresses > 0 {
            tiles.push(note(&words::local_addresses(report.local_addresses)));
        }
        tiles
    };
    sections.push(section(
        Some(words::NETWORK_ACTIVITY),
        Some(words::NETWORK_ACTIVITY_DETAIL),
        domains,
    ));
    if !report.questions.is_empty() {
        let tiles = report.questions.iter().map(|line| note(line)).collect();
        sections.push(section(Some(words::QUESTIONS), None, tiles));
    }
    let panels = Tab::ALL
        .into_iter()
        .filter(|tab| *tab != Tab::Report)
        .map(|tab| panel_tile(inspector, tab))
        .collect();
    sections.push(section(Some(words::LIVE), Some(words::LIVE_DETAIL), panels));
    let bar = CupertinoNavigationBar::new()
        .leading(back(words::PROGRAMS, inspector, |inspector, cx| {
            inspector.choose(None, cx)
        }))
        .middle(Text::new(seen.program.name.clone()));
    scaffold(bar, sections)
}

/// One kind of access: what and when, with a strip of the days it was had on; a tap opens the
/// panel that shows more of it, where one does.
fn access_tile(inspector: &Entity<Inspector>, access: &report::Access) -> WidgetRef {
    let mut said = access.said.clone();
    if let Some(last) = access.last {
        said = format!("{said} · last {}", words::ago(last));
    }
    let strip = Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(3.0)
        .children(access.days.iter().map(|used| {
            let fill = if *used {
                CupertinoColors::ACTIVE_BLUE
            } else {
                CupertinoColors::SYSTEM_FILL
            };
            Builder::new(move |app, context| {
                DecoratedBox::new(
                    BoxDecoration::new()
                        .color(resolved(fill.clone(), app, context))
                        .border_radius(BorderRadiusGeometry::circular(2.0)),
                )
                .child(SizedBox::new().width(STRIP_CELL).height(STRIP_CELL))
                .into_widget()
            })
            .into_widget()
        }));
    let tile = CupertinoListTile::new(Text::new(access.kind))
        .subtitle(Text::new(said))
        .additional_info(strip);
    match Tab::of_access(access.kind) {
        Some(tab) => tile
            .trailing(CupertinoListTileChevron::new())
            .on_tap(opening(inspector, tab))
            .into_widget(),
        None => tile.into_widget(),
    }
}

/// A domain contacted: its name, how often and when last, over a bar of its share of the most
/// contacted's.
fn domain_tile(domain: &report::Contacted) -> WidgetRef {
    let mut said = words::contacted(domain.times, domain.refused);
    if let Some(last) = domain.last {
        said = format!("{said} · last {}", words::ago(last));
    }
    let share = domain.share;
    let bar = LayoutBuilder::new(move |app, context, constraints| {
        let width = constraints.max_width;
        let rounded = |color, width: f64| {
            DecoratedBox::new(
                BoxDecoration::new()
                    .color(color)
                    .border_radius(BorderRadiusGeometry::circular(2.0)),
            )
            .child(SizedBox::new().width(width).height(4.0))
            .into_widget()
        };
        let track = resolved(CupertinoColors::SYSTEM_FILL, app, context);
        let fill = resolved(CupertinoColors::ACTIVE_BLUE, app, context);
        Stack::new()
            .children([
                rounded(track, width),
                rounded(fill, (width * share).max(4.0)),
            ])
            .into_widget()
    });
    let below = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(4.0)
        .children([bar.into_widget(), Text::new(said).into_widget()]);
    CupertinoListTile::new(Text::new(domain.domain.clone()))
        .subtitle(below)
        .padding(TALL_TILE_PADDING)
        .into_widget()
}

/// A live panel's tile, saying what it shows, which opens it.
fn panel_tile(inspector: &Entity<Inspector>, tab: Tab) -> WidgetRef {
    CupertinoListTile::new(Text::new(tab.name()))
        .subtitle(Text::new(tab.shows()))
        .trailing(CupertinoListTileChevron::new())
        .on_tap(opening(inspector, tab))
        .into_widget()
}

/// What a tap that opens `tab`'s panel does.
fn opening(inspector: &Entity<Inspector>, tab: Tab) -> AsyncCallback {
    let inspector = inspector.clone();
    Rc::new(move |app: &mut App| {
        show_tab(app, &inspector, tab);
        Task::ready(())
    })
}
