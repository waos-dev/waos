//! The report tab, as D1 draws it, Windows Settings' cards for an auditor: what WaOS refused over
//! the week, summed in one bar; each kind of access the program used, a card row each, with a
//! strip of the week's days, bars by count with refusals stacked in red, and a way on to its live
//! panel; the kinds it did not use, in one quiet line; the domains it contacted, in a table; and
//! the questions it put.
//!
//! The invariant: the report draws what `Report` says and nothing else, so both kits say the same.

use waft::{
    App, BorderRadiusGeometry, BoxDecoration, Column, CrossAxisAlignment, DecoratedBox,
    EdgeInsetsGeometry, Entity, Expanded, IntoWidget, ListView, Listener, MainAxisAlignment,
    MainAxisSize, Padding, Row, SizedBox, Text, TextStyle, WidgetRef,
};
use waft_winui::{
    Brush, Button, ButtonStyle, ControlBorder, FluentSymbol, FontIcon, InfoBar, InfoBarSeverity,
    TextBlockStyle, ThemeResources,
};
use waos_activity::AccessKind;

use super::table::Table;
use super::{GAP, MARGIN, caption, line, paragraph, symbol};
use crate::clock;
use crate::logic::detail::Glyph;
use crate::logic::report::{self, Report, Used};
use crate::state::page::{ProgramPage, Tab, show_tab};
use crate::words;

/// The tallest a day's bar is.
const BAR_HEIGHT: f64 = 18.0;
const BAR_WIDTH: f64 = 6.0;
/// The most domains the report's table shows before it scrolls.
const DOMAINS_SHOWN: usize = 8;

/// The report `report`.
pub(super) fn report(
    app: &mut App,
    page: &Entity<ProgramPage>,
    report: &Report,
    resources: &ThemeResources,
) -> WidgetRef {
    let mut parts = Vec::new();
    if let Some(bar) = refusals(report) {
        parts.push(bar);
    }
    parts.push(heading(
        words::DATA_AND_SENSOR_ACCESS,
        &words::last_seven_days(app),
        resources,
    ));
    parts.push(access_card(page, app, report, resources));
    parts.push(heading(
        words::NETWORK_ACTIVITY,
        words::NETWORK_ACTIVITY_DETAIL,
        resources,
    ));
    if report.domains.is_empty() {
        parts.push(paragraph(words::NO_DOMAINS, resources));
    } else {
        parts.push(domains(app, page, report.domains.len(), resources));
    }
    if report.local_addresses > 0 {
        parts.push(caption(
            &words::local_addresses(report.local_addresses),
            resources,
        ));
    }
    if !report.questions.is_empty() {
        parts.push(heading(words::QUESTIONS, "", resources));
        parts.extend(
            report
                .questions
                .iter()
                .map(|line| paragraph(line, resources)),
        );
    }
    ListView::new()
        .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, GAP, MARGIN, MARGIN))
        .children(parts)
        .into_widget()
}

/// What WaOS refused over the week, in one bar; none where it refused nothing.
fn refusals(report: &Report) -> Option<WidgetRef> {
    let refused = report.refused();
    if refused == 0 {
        return None;
    }
    let kinds: Vec<(&str, u32)> = (report.used.iter())
        .map(|used| (used.kind.name(), used.total().refused))
        .filter(|(_, refused)| *refused > 0)
        .collect();
    Some(
        Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, GAP))
            .child(
                InfoBar::new(true, |_app, _open| {})
                    .severity(InfoBarSeverity::Warning)
                    .is_closable(false)
                    .title(words::refused_this_week(refused))
                    .message(words::refused_kinds(&kinds)),
            )
            .into_widget(),
    )
}

/// A section's title, with what it holds beside it.
fn heading(title: &str, detail: &str, resources: &ThemeResources) -> WidgetRef {
    Padding::new(EdgeInsetsGeometry::only(0.0, GAP, 0.0, 8.0))
        .child(Row::new().spacing(GAP).children([
            line(
                title,
                TextBlockStyle::BodyStrong,
                resources.text_fill_color_primary,
                resources,
            ),
            Expanded::new(caption(detail, resources)).into_widget(),
        ]))
        .into_widget()
}

/// The card of the kinds of access used, a row each, and the kinds not used beneath them.
fn access_card(
    page: &Entity<ProgramPage>,
    app: &App,
    report: &Report,
    resources: &ThemeResources,
) -> WidgetRef {
    let mut rows: Vec<WidgetRef> = Vec::new();
    if report.is_empty() {
        rows.push(
            Padding::new(EdgeInsetsGeometry::all(GAP + 4.0))
                .child(paragraph(words::USED_NOTHING, resources))
                .into_widget(),
        );
    }
    for used in &report.used {
        if !rows.is_empty() {
            rows.push(divider(resources));
        }
        rows.push(used_row(page, app, used, resources));
    }
    if !report.unused.is_empty() {
        rows.push(divider(resources));
        rows.push(unused_row(&report.unused, resources));
    }
    ControlBorder::new(
        Brush::Solid(resources.card_background_fill_color_default),
        Brush::Solid(resources.card_stroke_color_default),
    )
    .border_thickness(1.0)
    .corner_radius(8.0)
    .child(
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(rows),
    )
    .into_widget()
}

/// One kind of access used: its glyph, what it did, the week's strip, when it last did, and the
/// way on to its panel.
fn used_row(
    page: &Entity<ProgramPage>,
    app: &App,
    used: &Used,
    resources: &ThemeResources,
) -> WidgetRef {
    let text = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children([
            line(
                used.kind.name(),
                TextBlockStyle::BodyStrong,
                resources.text_fill_color_primary,
                resources,
            ),
            caption(&used.said, resources),
        ]);
    let last = used
        .last
        .map(|last| format!("{} {}", words::LAST_LOWER, words::ago(last)))
        .unwrap_or_default();
    let row = Row::new()
        .spacing(GAP + 4.0)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children([
            glyph_tile(Glyph::of_access(used.kind), resources),
            Expanded::new(text).into_widget(),
            strip(used, app, resources),
            SizedBox::new()
                .width(96.0)
                .child(caption(&last, resources))
                .into_widget(),
            FontIcon::symbol(FluentSymbol::ChevronRight)
                .font_size(12.0)
                .foreground(resources.text_fill_color_secondary)
                .into_widget(),
        ]);
    let (page, tab) = (page.clone(), Tab::of_access(used.kind));
    Button::new(
        Padding::new(EdgeInsetsGeometry::symmetric(6.0, 4.0)).child(row),
        Listener::new(move |app| show_tab(app, &page, tab)),
    )
    .style(ButtonStyle::Subtle)
    .into_widget()
}

/// The kinds not used, in one quiet line.
fn unused_row(unused: &[AccessKind], resources: &ThemeResources) -> WidgetRef {
    let quiet = resources.text_fill_color_secondary;
    let mut items = vec![caption(words::NOT_USED, resources)];
    items.extend(unused.iter().map(|kind| {
        Row::new()
            .main_axis_size(MainAxisSize::Min)
            .spacing(4.0)
            .children([
                FontIcon::symbol(symbol(Glyph::of_access(*kind)))
                    .font_size(12.0)
                    .foreground(quiet)
                    .into_widget(),
                caption(kind.name(), resources),
            ])
            .into_widget()
    }));
    Padding::new(EdgeInsetsGeometry::symmetric(10.0, GAP + 4.0))
        .child(Row::new().spacing(GAP).children(items))
        .into_widget()
}

/// A glyph in a small rounded square, as a settings card leads its row.
fn glyph_tile(glyph: Glyph, resources: &ThemeResources) -> WidgetRef {
    ControlBorder::new(
        Brush::Solid(resources.subtle_fill_color_secondary),
        Brush::Solid(resources.card_stroke_color_default),
    )
    .border_thickness(0.0)
    .corner_radius(6.0)
    .child(
        SizedBox::new().width(32.0).height(32.0).child(
            waft::Center::new().child(
                FontIcon::symbol(symbol(glyph))
                    .font_size(16.0)
                    .foreground(resources.text_fill_color_primary),
            ),
        ),
    )
    .into_widget()
}

/// The week's days, oldest first: a bar each by how often, its refusals stacked in red on top,
/// over each day's initial. Drawn until Waft has a chart.
fn strip(used: &Used, app: &App, resources: &ThemeResources) -> WidgetRef {
    let most = (used.days.iter())
        .map(|day| day.allowed + day.refused)
        .max()
        .unwrap_or(0)
        .max(1);
    let height = |count: u32| f64::from(count) / f64::from(most) * BAR_HEIGHT;
    let initial = TextStyle::new()
        .font_size(9.0)
        .color(resources.text_fill_color_tertiary);
    let days = used.days.iter().enumerate().map(|(slot, day)| {
        let mut bar: Vec<WidgetRef> = Vec::new();
        if day.refused > 0 {
            bar.push(block(
                height(day.refused),
                resources.system_fill_color_critical,
            ));
        }
        if day.allowed > 0 {
            bar.push(block(
                height(day.allowed),
                resources.accent_fill_color_default_brush,
            ));
        }
        if bar.is_empty() {
            bar.push(block(2.0, resources.control_strong_fill_color_disabled));
        }
        let ago = report::DAYS - 1 - slot;
        Column::new()
            .main_axis_size(MainAxisSize::Min)
            .spacing(2.0)
            .children([
                SizedBox::new()
                    .width(BAR_WIDTH + 2.0)
                    .height(BAR_HEIGHT)
                    .child(
                        Column::new()
                            .main_axis_alignment(MainAxisAlignment::End)
                            .children(bar),
                    )
                    .into_widget(),
                Text::new(clock::weekday_initial(app, ago))
                    .style(initial.clone())
                    .into_widget(),
            ])
            .into_widget()
    });
    Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(3.0)
        .children(days.collect::<Vec<_>>())
        .into_widget()
}

fn block(height: f64, color: waft::Color) -> WidgetRef {
    DecoratedBox::new(
        BoxDecoration::new()
            .color(color)
            .border_radius(BorderRadiusGeometry::circular(1.5)),
    )
    .child(SizedBox::new().width(BAR_WIDTH).height(height))
    .into_widget()
}

/// The domains contacted, in a table as tall as its rows, up to a few.
fn domains(
    app: &mut App,
    page: &Entity<ProgramPage>,
    count: usize,
    resources: &ThemeResources,
) -> WidgetRef {
    let rows = page.read(app).domains.clone();
    // The header and each row take a compact row's height and its line.
    let row = resources.table_view_row_min_height_compact + 1.0;
    let shown = count.min(DOMAINS_SHOWN) as f64;
    SizedBox::new()
        .height(row * (shown + 1.0) + 2.0)
        .child(Table {
            rows,
            selection: None,
            empty: words::NO_DOMAINS,
            narrowed: false,
        })
        .into_widget()
}

fn divider(resources: &ThemeResources) -> WidgetRef {
    DecoratedBox::new(BoxDecoration::new().color(resources.divider_stroke_color_default))
        .child(SizedBox::new().height(1.0))
        .into_widget()
}
