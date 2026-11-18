//! The app at a desktop's or a tablet's width, in Fluent's controls, as the inspector design's
//! mockups draw it: the programs in a side pane, as
//! Windows Settings lists its pages, beside the page of the program chosen: its header with its
//! commands on the right, its report first and its live panels in tabs after it, as WinUI's
//! `SelectorBar` switches a page's views.
//!
//! The invariant: every part is drawn from the entities alone, and calls them for every change, so
//! nothing here holds what the app knows.

mod commands;
mod detail;
mod panel;
mod program_list;
mod report;
mod storage;
mod table;

use std::rc::Rc;

use waft::{
    App, BuildContext, Center, ColoredBox, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity,
    Expanded, IntoWidget, MainAxisSize, Padding, PageRouteBuilder, RouteSettingsRef, Row, SizedBox,
    StatelessWidget, Text, TextOverflow, TextStyle, WidgetRef, WidgetsApp,
};
use waft_winui::{
    AccentPalette, Brush, ControlBorder, FluentSymbol, FontIcon, InfoBadge, InfoBadgeStyle,
    ProgressRing, SelectorBar, SelectorBarItem, TextBlockStyle, ThemeResources,
};

use crate::clock;
use crate::code_font::CODE_FONT;
use crate::logic::detail::Glyph;
use crate::logic::panels::Tone;
use crate::logic::report::Report;
use crate::program_icon::ProgramIcon;
use crate::state::page::{ProgramPage, Tab, show_tab};
use crate::state::programs::{Found, Programs, Seen};
use crate::words;
use program_list::ProgramList;

const MARGIN: f64 = 24.0;
const GAP: f64 = 12.0;
const SIDE_PANE: f64 = 280.0;
const HEADER_ICON: f64 = 40.0;
/// The size of the glyphs on rows and in the toolbar.
const SMALL_GLYPH: f64 = 14.0;

/// The app in Fluent's controls, in its own app of Fluent's accent.
#[derive(Debug)]
pub(crate) struct FluentInspector {
    pub(crate) page: Entity<ProgramPage>,
}

impl StatelessWidget for FluentInspector {
    fn build(&self, _app: &mut App, _context: BuildContext) -> WidgetRef {
        WidgetsApp::new(AccentPalette::default().base)
            .title(words::APP_NAME)
            .debug_show_checked_mode_banner(false)
            .page_route_builder(|app, settings, builder| {
                let route = PageRouteBuilder::new(
                    app,
                    Rc::new(move |app, context, _, _| builder(app, context)),
                )
                .settings(app, RouteSettingsRef::Settings(settings.clone()));
                route.into_erased()
            })
            .home(FluentPage {
                page: self.page.clone(),
            })
            .into_widget()
    }
}

/// The side pane of programs beside the page of the one chosen, on Fluent's background.
#[derive(Debug)]
struct FluentPage {
    page: Entity<ProgramPage>,
}

impl StatelessWidget for FluentPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let programs = self.page.read(app).programs.clone();
        let content = match programs.read(app).found {
            Found::Looking => Center::new()
                .child(ProgressRing::new().is_active(true))
                .into_widget(),
            Found::NoDevice => said(words::NO_DEVICE, &resources),
            Found::Ready => Row::new()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children([
                    SizedBox::new()
                        .width(SIDE_PANE)
                        .child(ProgramList { programs })
                        .into_widget(),
                    Expanded::new(content_layer(
                        ProgramPane {
                            page: self.page.clone(),
                        }
                        .into_widget(),
                        &resources,
                    ))
                    .into_widget(),
                ])
                .into_widget(),
        };
        ColoredBox::new(resources.solid_background_fill_color_base)
            .child(content)
            .into_widget()
    }
}

/// The page's layer beside the side pane, as WinUI's `NavigationView` lays its content: a lighter
/// fill, rounded at its top left.
fn content_layer(child: WidgetRef, resources: &ThemeResources) -> WidgetRef {
    ControlBorder::new(
        Brush::Solid(resources.layer_fill_color_default),
        Brush::Solid(resources.card_stroke_color_default),
    )
    .border_thickness_ltrb([1.0, 1.0, 0.0, 0.0])
    .corner_radius_corners([8.0, 0.0, 0.0, 0.0])
    .child(child)
    .into_widget()
}

/// The page of the program chosen: its header and commands, its tabs, and the tab shown.
#[derive(Debug)]
struct ProgramPane {
    page: Entity<ProgramPage>,
}

impl StatelessWidget for ProgramPane {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let programs = self.page.read(app).programs.clone();
        let Some(seen) = programs.read(app).chosen(app).cloned() else {
            return said(words::CHOOSE, &resources);
        };
        let tab = self.page.read(app).tab;
        let content = match (tab, self.page.read(app).panels()) {
            (Tab::Report, _) | (_, None) => {
                let report = Report::of(&seen.days, clock::today(app));
                report::report(app, &self.page, &report, &resources)
            }
            (Tab::Storage, _) => storage::storage(app, &self.page, &resources),
            (tab, Some(panels)) => panel::panel_of(panels, tab, &seen),
        };
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, MARGIN, MARGIN, GAP))
                    .child(header(app, &self.page, &programs, &seen, &resources))
                    .into_widget(),
                Padding::new(EdgeInsetsGeometry::from_ltrb(
                    MARGIN - 8.0,
                    0.0,
                    MARGIN,
                    0.0,
                ))
                .child(tabs(app, &self.page, tab))
                .into_widget(),
                Expanded::new(content).into_widget(),
            ])
            .into_widget()
    }
}

/// The program's icon, name and id, whether it runs, and its commands on the right.
fn header(
    app: &mut App,
    page: &Entity<ProgramPage>,
    programs: &Entity<Programs>,
    seen: &Seen,
    resources: &ThemeResources,
) -> WidgetRef {
    let icon = ProgramIcon {
        png: programs.read(app).icon(&seen.program.id),
        side: HEADER_ICON,
    };
    let name = Text::new(seen.program.name.clone())
        .style(TextBlockStyle::Subtitle.text_style(resources, resources.text_fill_color_primary));
    let running = if seen.program.running {
        words::RUNNING
    } else {
        words::NOT_RUNNING_SHORT
    };
    let status = Row::new()
        .main_axis_size(MainAxisSize::Min)
        .spacing(6.0)
        .children([
            code_line(
                &seen.program.id,
                TextBlockStyle::Caption,
                resources.text_fill_color_secondary,
                resources,
            ),
            caption("·", resources),
            status_dot(seen.program.running, resources),
            caption(running, resources),
        ]);
    let text = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children([name.into_widget(), status.into_widget()]);
    Row::new()
        .spacing(GAP)
        .children([
            icon.into_widget(),
            Expanded::new(text).into_widget(),
            commands::commands(page, seen),
        ])
        .into_widget()
}

/// The page's tabs, as a selector bar: the report, then each live panel, each with its glyph and
/// how many rows its list holds in a badge beside its name, critical where any of them were
/// refused, and none while the list is empty.
fn tabs(app: &mut App, page: &Entity<ProgramPage>, shown: Tab) -> WidgetRef {
    let counts: Option<[Option<(usize, usize)>; Tab::ALL.len()]> = page
        .read(app)
        .panels()
        .map(|panels| Tab::ALL.map(|tab| panels.counted(app, tab)));
    let items = Tab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            let counted = counts
                .as_ref()
                .and_then(|counts| counts[index])
                .filter(|(rows, _)| *rows > 0);
            let mut label = vec![Text::new(tab.name()).into_widget()];
            label.extend(counted.map(|(rows, refused)| {
                let style = if refused > 0 {
                    InfoBadgeStyle::Critical
                } else {
                    InfoBadgeStyle::Informational
                };
                InfoBadge::new()
                    .value(i32::try_from(rows).unwrap_or(i32::MAX))
                    .style(style)
                    .into_widget()
            }));
            let label = Row::new()
                .main_axis_size(MainAxisSize::Min)
                .spacing(6.0)
                .children(label);
            SelectorBarItem::with_child(tab.name(), label).icon(FontIcon::symbol(tab_symbol(*tab)))
        })
        .collect();
    let selected = Tab::ALL.iter().position(|tab| *tab == shown);
    let page = page.clone();
    SelectorBar::new(items, selected, move |app, asked| {
        if let Some(tab) = asked.and_then(|index| Tab::ALL.get(index)) {
            show_tab(app, &page, *tab);
        }
    })
    .into_widget()
}

fn tab_symbol(tab: Tab) -> FluentSymbol {
    match tab {
        Tab::Report => FluentSymbol::Shield,
        Tab::Network => FluentSymbol::Globe,
        Tab::Files => FluentSymbol::Folder,
        Tab::Storage => FluentSymbol::Folder,
        Tab::Access => FluentSymbol::Camera,
        Tab::Output => FluentSymbol::WindowConsole,
        Tab::InUse => FluentSymbol::Pulse,
    }
}

/// The Fluent symbol a glyph is drawn with.
pub(super) fn symbol(glyph: Glyph) -> FluentSymbol {
    match glyph {
        Glyph::Request | Glyph::Datagram => FluentSymbol::Globe,
        Glyph::Connection => FluentSymbol::PlugConnected,
        Glyph::Lookup => FluentSymbol::Search,
        Glyph::Listener => FluentSymbol::Live,
        Glyph::File => FluentSymbol::Document,
        Glyph::Folder => FluentSymbol::Folder,
        Glyph::Watch => FluentSymbol::Eye,
        Glyph::Camera => FluentSymbol::Camera,
        Glyph::Microphone => FluentSymbol::Mic,
        Glyph::Clipboard => FluentSymbol::Clipboard,
        Glyph::Link => FluentSymbol::Link,
        Glyph::Notification => FluentSymbol::Alert,
        Glyph::Question => FluentSymbol::Question,
        Glyph::Machine => FluentSymbol::Desktop,
        Glyph::Output => FluentSymbol::WindowConsole,
    }
}

/// `glyph` at the rows' size, in `color`.
pub(super) fn glyph(glyph: Glyph, color: waft::Color) -> WidgetRef {
    FontIcon::symbol(symbol(glyph))
        .font_size(SMALL_GLYPH)
        .foreground(color)
        .into_widget()
}

/// The colour a tone reads in.
pub(super) fn toned(tone: Tone, resources: &ThemeResources) -> waft::Color {
    match tone {
        Tone::Plain => resources.text_fill_color_primary,
        Tone::Quiet => resources.text_fill_color_secondary,
        Tone::Allowed => resources.system_fill_color_success,
        Tone::Refused => resources.system_fill_color_critical,
    }
}

/// A dot saying whether a program runs: green while it does.
pub(super) fn status_dot(running: bool, resources: &ThemeResources) -> WidgetRef {
    let color = if running {
        resources.system_fill_color_success
    } else {
        resources.text_fill_color_tertiary
    };
    ControlBorder::new(Brush::Solid(color), Brush::Solid(color))
        .border_thickness(0.0)
        .corner_radius(4.0)
        .child(SizedBox::new().width(8.0).height(8.0))
        .into_widget()
}

/// `text` on one line in `style` and `color`, cut short where it is too long.
pub(super) fn line(
    text: &str,
    style: TextBlockStyle,
    color: waft::Color,
    resources: &ThemeResources,
) -> WidgetRef {
    Text::new(text.to_owned())
        .style(style.text_style(resources, color))
        .max_lines(1)
        .overflow(TextOverflow::Ellipsis)
        .into_widget()
}

/// `style` in `color`, in the code font: for URLs, paths, addresses, times and sizes.
pub(super) fn code_style(
    style: TextBlockStyle,
    color: waft::Color,
    resources: &ThemeResources,
) -> TextStyle {
    style.text_style(resources, color).font_family(CODE_FONT)
}

/// `text` on one line in the code font, in `style` and `color`, cut short where it is too long.
pub(super) fn code_line(
    text: &str,
    style: TextBlockStyle,
    color: waft::Color,
    resources: &ThemeResources,
) -> WidgetRef {
    Text::new(text.to_owned())
        .style(code_style(style, color, resources))
        .max_lines(1)
        .overflow(TextOverflow::Ellipsis)
        .into_widget()
}

/// `text` on one line, in the caption size, secondary.
pub(super) fn caption(text: &str, resources: &ThemeResources) -> WidgetRef {
    line(
        text,
        TextBlockStyle::Caption,
        resources.text_fill_color_secondary,
        resources,
    )
}

/// `text` in the body size, as many lines as it takes.
pub(super) fn paragraph(text: &str, resources: &ThemeResources) -> WidgetRef {
    let style = TextBlockStyle::Body.text_style(resources, resources.text_fill_color_primary);
    Text::new(text.to_owned()).style(style).into_widget()
}

/// `text` alone, in the middle of what it fills.
pub(super) fn said(text: &str, resources: &ThemeResources) -> WidgetRef {
    Center::new()
        .child(Padding::new(EdgeInsetsGeometry::all(MARGIN)).child(
            Text::new(text.to_owned()).style(
                TextBlockStyle::Body.text_style(resources, resources.text_fill_color_secondary),
            ),
        ))
        .into_widget()
}
