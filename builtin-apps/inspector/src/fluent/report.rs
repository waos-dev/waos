//! The report tab: what the program did over the last seven days, as iOS's App Privacy Report and
//! Android's Privacy dashboard show an app's: each kind of access with a strip of the days it was
//! had on, the domains contacted ranked with a bar each, the questions put; and a way on to each
//! live panel.
//!
//! The invariant: the report draws what `Report` says and nothing else, so both kits say the same.

use waft::{
    Align, AlignmentGeometry, Border, BorderRadiusGeometry, BorderSide, BorderStyle, BoxDecoration,
    Center, Column, CrossAxisAlignment, DecoratedBox, EdgeInsetsGeometry, Entity, Expanded,
    IntoWidget, LayoutBuilder, ListView, Listener, MainAxisSize, Padding, Row, SizedBox, Stack,
    Text, WidgetRef,
};
use waft_winui::{HyperlinkButton, TextBlockStyle, ThemeResources};

use super::{GAP, MARGIN, ROW_PADDING, one_line, paragraph};
use crate::clock;
use crate::inspector::{Inspector, Seen, Tab, show_tab};
use crate::report::{self, Report};
use crate::words;

/// A day's cell in an access's strip, and the gap after it.
const DAY_CELL: f64 = 14.0;
const DAY_GAP: f64 = 4.0;
/// The height of a domain's bar.
const BAR: f64 = 4.0;
/// The width of an access's name before its strip.
const ACCESS_NAME: f64 = 120.0;

/// The report: each kind of access with the days it was had on, the domains contacted ranked,
/// the questions put, and a way on to each live panel.
pub(super) fn report(
    inspector: &Entity<Inspector>,
    seen: &Seen,
    resources: &ThemeResources,
) -> WidgetRef {
    let report = Report::of(&seen.days, clock::today());
    let mut rows = Vec::new();
    if report.is_empty() {
        rows.push(paragraph(words::USED_NOTHING, resources));
    }
    if !report.access.is_empty() {
        rows.extend(heading(
            words::ACCESS,
            Some(words::ACCESS_DETAIL),
            resources,
        ));
        rows.push(days_header(resources));
        rows.extend(
            report
                .access
                .iter()
                .map(|access| access_row(access, resources)),
        );
    }
    rows.extend(heading(
        words::NETWORK_ACTIVITY,
        Some(words::NETWORK_ACTIVITY_DETAIL),
        resources,
    ));
    if report.domains.is_empty() {
        rows.push(paragraph(words::NO_DOMAINS, resources));
    }
    rows.extend(
        report
            .domains
            .iter()
            .map(|domain| domain_row(domain, resources)),
    );
    if report.local_addresses > 0 {
        let said = words::local_addresses(report.local_addresses);
        rows.push(paragraph(&said, resources));
    }
    if !report.questions.is_empty() {
        rows.extend(heading(words::QUESTIONS, None, resources));
        rows.extend(
            report
                .questions
                .iter()
                .map(|line| paragraph(line, resources)),
        );
    }
    rows.extend(heading(words::LIVE, Some(words::LIVE_DETAIL), resources));
    rows.extend(Tab::ALL[1..].iter().map(|tab| panel_link(inspector, *tab)));
    ListView::new()
        .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, MARGIN))
        .children(rows)
        .into_widget()
}

/// A section's title, and what it holds where said.
fn heading(title: &str, detail: Option<&str>, resources: &ThemeResources) -> Vec<WidgetRef> {
    let primary = resources.text_fill_color_primary;
    let title = Text::new(title.to_owned())
        .style(TextBlockStyle::BodyStrong.text_style(resources, primary));
    let mut rows = vec![
        Padding::new(EdgeInsetsGeometry::only(0.0, MARGIN, 0.0, 2.0))
            .child(title)
            .into_widget(),
    ];
    if let Some(detail) = detail {
        rows.push(
            Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, GAP / 2.0))
                .child(one_line(detail, TextBlockStyle::Caption, resources, false))
                .into_widget(),
        );
    }
    rows
}

/// The days the strips show, oldest first, by their weekdays' initials.
fn days_header(resources: &ThemeResources) -> WidgetRef {
    let initials = (0..report::DAYS).rev().map(|ago| {
        Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, DAY_GAP, 0.0))
            .child(
                SizedBox::new()
                    .width(DAY_CELL)
                    .child(Center::new().child(one_line(
                        clock::weekday_initial(ago),
                        TextBlockStyle::Caption,
                        resources,
                        false,
                    ))),
            )
            .into_widget()
    });
    let mut cells = vec![SizedBox::new().width(ACCESS_NAME).into_widget()];
    cells.extend(initials);
    Padding::new(EdgeInsetsGeometry::only(0.0, GAP, 0.0, 4.0))
        .child(Row::new().children(cells))
        .into_widget()
}

/// One kind of access: its name, a strip of the days it was had on, and what and when.
fn access_row(access: &report::Access, resources: &ThemeResources) -> WidgetRef {
    let strip = access.days.iter().map(|used| {
        let cell = BoxDecoration::new().border_radius(BorderRadiusGeometry::circular(3.0));
        let cell = if *used {
            cell.color(resources.accent_fill_color_default_brush)
        } else {
            let outline = resources.control_strong_fill_color_disabled;
            cell.border(Border::from_border_side(BorderSide::new(
                outline,
                1.0,
                BorderStyle::Solid,
                -1.0,
            )))
        };
        Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, DAY_GAP, 0.0))
            .child(DecoratedBox::new(cell).child(SizedBox::new().width(DAY_CELL).height(DAY_CELL)))
            .into_widget()
    });
    let mut said = access.said.clone();
    if let Some(last) = access.last {
        said = format!("{said} · last {}", words::ago(last));
    }
    let mut cells = vec![
        SizedBox::new()
            .width(ACCESS_NAME)
            .child(one_line(access.kind, TextBlockStyle::Body, resources, true))
            .into_widget(),
    ];
    cells.extend(strip);
    cells.push(
        Expanded::new(
            Padding::new(EdgeInsetsGeometry::only(GAP, 0.0, 0.0, 0.0)).child(one_line(
                &said,
                TextBlockStyle::Caption,
                resources,
                false,
            )),
        )
        .into_widget(),
    );
    Padding::new(EdgeInsetsGeometry::symmetric(ROW_PADDING, 0.0))
        .child(
            Row::new()
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(cells),
        )
        .into_widget()
}

/// A domain contacted: its name, a bar of its share of the most contacted's, and how often.
fn domain_row(domain: &report::Contacted, resources: &ThemeResources) -> WidgetRef {
    let bar = LayoutBuilder::new({
        let (share, fill, track) = (
            domain.share,
            resources.accent_fill_color_default_brush,
            resources.subtle_fill_color_secondary,
        );
        move |_app, _context, constraints| {
            let width = constraints.max_width;
            let rounded = |color, width: f64| {
                DecoratedBox::new(
                    BoxDecoration::new()
                        .color(color)
                        .border_radius(BorderRadiusGeometry::circular(BAR / 2.0)),
                )
                .child(SizedBox::new().width(width).height(BAR))
                .into_widget()
            };
            Stack::new()
                .children([
                    rounded(track, width),
                    rounded(fill, (width * share).max(BAR)),
                ])
                .into_widget()
        }
    });
    let mut said = words::contacted(domain.times, domain.refused);
    if let Some(last) = domain.last {
        said = format!("{said} · last {}", words::ago(last));
    }
    let text = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .spacing(4.0)
        .children([
            Row::new()
                .children([
                    Expanded::new(one_line(
                        &domain.domain,
                        TextBlockStyle::Body,
                        resources,
                        true,
                    ))
                    .into_widget(),
                    one_line(&said, TextBlockStyle::Caption, resources, false),
                ])
                .into_widget(),
            bar.into_widget(),
        ]);
    Padding::new(EdgeInsetsGeometry::symmetric(ROW_PADDING, 0.0))
        .child(text)
        .into_widget()
}

/// The way from the report to `tab`'s panel, saying what it shows.
fn panel_link(inspector: &Entity<Inspector>, tab: Tab) -> WidgetRef {
    let inspector = inspector.clone();
    let open = Listener::new(move |app| show_tab(app, &inspector, tab));
    Align::new()
        .alignment(AlignmentGeometry::CENTER_START)
        .child(HyperlinkButton::text(tab.shows(), open))
        .into_widget()
}
