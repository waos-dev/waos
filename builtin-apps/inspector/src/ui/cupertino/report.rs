//! The report page, as P1 draws it after iOS's App Privacy Report: the program's header; what
//! WaOS refused over the week, said once; each kind of access it used with a strip of the week's
//! days, bars by count with refusals in red, a tap opening its panel; the kinds it did not use,
//! in one line; the domains it contacted with how often; the questions it put; and its live
//! panels, a tile each.
//!
//! The invariant: the page draws what `Report` says and nothing else, so both kits say the same.

use std::rc::Rc;

use waft::{
    App, AsyncCallback, BorderRadiusGeometry, BoxDecoration, Builder, Column, DecoratedBox, Entity,
    IntoWidget, MainAxisAlignment, MainAxisSize, Row, SizedBox, Task, Text, WidgetRef,
};
use waft_cupertino::{
    CupertinoColors, CupertinoListTile, CupertinoListTileChevron, CupertinoNavigationBar,
};

use super::{ICON_SIDE, TALL_TILE_PADDING, back, note, resolved, scaffold, section};
use crate::clock;
use crate::logic::report::{Contacted, Report, Used};
use crate::program_icon::ProgramIcon;
use crate::state::page::{ProgramPage, Tab, show_tab};
use crate::state::programs::{Seen, choose};
use crate::words;

/// The tallest a day's bar is.
const BAR_HEIGHT: f64 = 14.0;
const BAR_WIDTH: f64 = 5.0;

/// The program's report.
pub(super) fn report_page(page: &Entity<ProgramPage>, seen: &Seen, app: &mut App) -> WidgetRef {
    let programs = page.read(app).programs.clone();
    let icon = ProgramIcon {
        png: programs.read(app).icon(&seen.program.id),
        side: ICON_SIDE,
    };
    let header = CupertinoListTile::new(Text::new(seen.program.name.clone()))
        .subtitle(Text::new(words::detail(seen)))
        .leading(icon)
        .leading_size(ICON_SIDE)
        .into_widget();
    let report = Report::of(&seen.days, clock::today());
    let mut sections = vec![section(None, None, vec![header])];
    let refused = report.refused();
    if refused > 0 {
        sections.push(section(
            None,
            None,
            vec![note(&words::refused_this_week(refused))],
        ));
    }
    let mut tiles: Vec<WidgetRef> = if report.is_empty() {
        vec![note(words::USED_NOTHING)]
    } else {
        (report.used.iter())
            .map(|used| access_tile(page, used))
            .collect()
    };
    if !report.unused.is_empty() {
        let unused: Vec<&str> = report.unused.iter().map(|kind| kind.name()).collect();
        tiles.push(note(&format!("{} {}", words::NOT_USED, unused.join(", "))));
    }
    sections.push(section(
        Some(words::DATA_AND_SENSOR_ACCESS),
        Some(&words::last_seven_days()),
        tiles,
    ));
    let mut domains: Vec<WidgetRef> = if report.domains.is_empty() {
        vec![note(words::NO_DOMAINS)]
    } else {
        report.domains.iter().map(domain_tile).collect()
    };
    if report.local_addresses > 0 {
        domains.push(note(&words::local_addresses(report.local_addresses)));
    }
    sections.push(section(
        Some(words::NETWORK_ACTIVITY),
        Some(words::NETWORK_ACTIVITY_DETAIL),
        domains,
    ));
    if !report.questions.is_empty() {
        let tiles = report.questions.iter().map(|line| note(line)).collect();
        sections.push(section(Some(words::QUESTIONS), None, tiles));
    }
    let panels = Tab::ALL[1..]
        .iter()
        .map(|tab| panel_tile(page, *tab))
        .collect();
    sections.push(section(Some(words::LIVE), Some(words::LIVE_DETAIL), panels));
    let bar = CupertinoNavigationBar::new()
        .leading(back(words::PROGRAMS, page, |app, page| {
            let programs = page.read(app).programs.clone();
            choose(app, &programs, None);
        }))
        .middle(Text::new(seen.program.name.clone()));
    scaffold(bar, sections)
}

/// One kind of access used: what and when, with the week's strip; a tap opens its panel.
fn access_tile(page: &Entity<ProgramPage>, used: &Used) -> WidgetRef {
    let mut said = used.said.clone();
    if let Some(last) = used.last {
        said = format!("{said} · {} {}", words::LAST_LOWER, words::ago(last));
    }
    CupertinoListTile::new(Text::new(used.kind.name()))
        .subtitle(Text::new(said))
        .additional_info(strip(used))
        .trailing(CupertinoListTileChevron::new())
        .on_tap(opening(page, Tab::of_access(used.kind)))
        .into_widget()
}

/// The week's days, oldest first: a bar each by how often, its refusals in red on top. Drawn
/// until Waft has a chart.
fn strip(used: &Used) -> WidgetRef {
    let most = (used.days.iter())
        .map(|day| day.allowed + day.refused)
        .max()
        .unwrap_or(0)
        .max(1);
    let days = used.days.iter().map(|day| {
        let height = |count: u32| f64::from(count) / f64::from(most) * BAR_HEIGHT;
        let mut bar = Vec::new();
        if day.refused > 0 {
            bar.push(block(height(day.refused), CupertinoColors::SYSTEM_RED));
        }
        if day.allowed > 0 {
            bar.push(block(height(day.allowed), CupertinoColors::ACTIVE_BLUE));
        }
        if bar.is_empty() {
            bar.push(block(2.0, CupertinoColors::SYSTEM_FILL));
        }
        SizedBox::new()
            .width(BAR_WIDTH)
            .height(BAR_HEIGHT)
            .child(
                Column::new()
                    .main_axis_alignment(MainAxisAlignment::End)
                    .children(bar),
            )
            .into_widget()
    });
    Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(2.0)
        .children(days.collect::<Vec<_>>())
        .into_widget()
}

fn block(height: f64, color: waft::AnyColor) -> WidgetRef {
    Builder::new(move |app, context| {
        DecoratedBox::new(
            BoxDecoration::new()
                .color(resolved(color.clone(), app, context))
                .border_radius(BorderRadiusGeometry::circular(1.0)),
        )
        .child(SizedBox::new().width(BAR_WIDTH).height(height))
        .into_widget()
    })
    .into_widget()
}

/// A domain contacted: its name, how often and when last.
fn domain_tile(domain: &Contacted) -> WidgetRef {
    let mut said = words::contacted(domain.times, domain.refused);
    if let Some(last) = domain.last {
        said = format!("{said} · {} {}", words::LAST_LOWER, words::ago(last));
    }
    CupertinoListTile::new(Text::new(domain.domain.clone()))
        .subtitle(Text::new(said))
        .padding(TALL_TILE_PADDING)
        .into_widget()
}

/// A live panel's tile, saying what it shows, which opens it.
fn panel_tile(page: &Entity<ProgramPage>, tab: Tab) -> WidgetRef {
    CupertinoListTile::new(Text::new(tab.name()))
        .subtitle(Text::new(tab.shows()))
        .trailing(CupertinoListTileChevron::new())
        .on_tap(opening(page, tab))
        .into_widget()
}

/// What a tap that opens `tab`'s panel does.
fn opening(page: &Entity<ProgramPage>, tab: Tab) -> AsyncCallback {
    let page = page.clone();
    Rc::new(move |app: &mut App| {
        show_tab(app, &page, tab);
        Task::ready(())
    })
}
