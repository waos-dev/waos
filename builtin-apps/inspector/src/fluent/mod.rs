//! The app at a desktop's or a tablet's width, in Fluent's controls: the programs in a selectable
//! list beside the page of the program selected, its report first and its live panels in tabs
//! after it, as WinUI's `SelectorBar` switches a page's views.
//!
//! The invariant: what shows beside the list is the program chosen, which the list's selection
//! chooses; each tab is drawn from the inspector's state alone.

mod panel;
mod program_list;
mod report;

use std::rc::Rc;

use waft::{
    App, BuildContext, Center, ColoredBox, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity,
    Expanded, IntoWidget, MainAxisSize, Padding, PageRouteBuilder, RouteSettingsRef, Row, SizedBox,
    StatelessWidget, Text, TextOverflow, WidgetRef, WidgetsApp,
};
use waft_winui::{
    AccentPalette, ProgressRing, SelectorBar, SelectorBarItem, TextBlockStyle, ThemeResources,
};

use crate::inspector::{Found, Inspector, Seen, Tab, show_tab};
use crate::program_icon::ProgramIcon;
use crate::words;
use program_list::ProgramList;

const MARGIN: f64 = 24.0;
const GAP: f64 = 12.0;
const LIST_WIDTH: f64 = 320.0;
const HEADER_ICON: f64 = 48.0;
/// The space above and below a row of the report or of a panel.
const ROW_PADDING: f64 = 6.0;

/// The app in Fluent's controls, in its own app of Fluent's accent.
#[derive(Debug)]
pub(crate) struct FluentInspector {
    pub(crate) inspector: Entity<Inspector>,
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
                inspector: self.inspector.clone(),
            })
            .into_widget()
    }
}

/// The list beside what the program chosen did, on Fluent's background.
#[derive(Debug)]
struct FluentPage {
    inspector: Entity<Inspector>,
}

impl StatelessWidget for FluentPage {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let content = match self.inspector.read(app).found {
            Found::Looking => Center::new()
                .child(ProgressRing::new().is_active(true))
                .into_widget(),
            Found::NoDevice => said(words::NO_DEVICE, &resources),
            Found::Ready => Row::new()
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .children([
                    SizedBox::new()
                        .width(LIST_WIDTH)
                        .child(ProgramList {
                            inspector: self.inspector.clone(),
                        })
                        .into_widget(),
                    Expanded::new(ProgramPane {
                        inspector: self.inspector.clone(),
                    })
                    .into_widget(),
                ])
                .into_widget(),
        };
        ColoredBox::new(resources.solid_background_fill_color_base)
            .child(content)
            .into_widget()
    }
}

/// The page of the program chosen: its header, its tabs, the report first and the live panels
/// after it, and the tab shown.
#[derive(Debug)]
struct ProgramPane {
    inspector: Entity<Inspector>,
}

impl StatelessWidget for ProgramPane {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let Some(seen) = self.inspector.read(app).chosen().cloned() else {
            return said(words::CHOOSE, &resources);
        };
        let tab = self.inspector.read(app).tab;
        let content = match self.inspector.read(app).live.panel(tab) {
            None => report::report(&self.inspector, &seen, &resources),
            Some(panel) => {
                let heard = self.inspector.read(app).live.heard;
                panel::panel(panel, heard, &resources)
            }
        };
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, MARGIN, MARGIN, GAP))
                    .child(self.header(app, &seen, &resources))
                    .into_widget(),
                Padding::new(EdgeInsetsGeometry::from_ltrb(
                    MARGIN - 8.0,
                    0.0,
                    MARGIN,
                    0.0,
                ))
                .child(tabs(&self.inspector, tab))
                .into_widget(),
                Expanded::new(content).into_widget(),
            ])
            .into_widget()
    }
}

impl ProgramPane {
    /// The program's icon and name, with what is said of it.
    fn header(&self, app: &mut App, seen: &Seen, resources: &ThemeResources) -> WidgetRef {
        let icon = ProgramIcon {
            image: self.inspector.read(app).icon(&seen.program.id),
            side: HEADER_ICON,
        };
        let primary = resources.text_fill_color_primary;
        let name = Text::new(seen.program.name.clone())
            .style(TextBlockStyle::Subtitle.text_style(resources, primary));
        let text = Column::new()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children([
                name.into_widget(),
                one_line(
                    &words::detail(seen),
                    TextBlockStyle::Caption,
                    resources,
                    false,
                ),
            ]);
        Row::new()
            .spacing(GAP)
            .children([icon.into_widget(), Expanded::new(text).into_widget()])
            .into_widget()
    }
}

/// The page's tabs, as a selector bar: the report, then each live panel.
fn tabs(inspector: &Entity<Inspector>, shown: Tab) -> WidgetRef {
    let items = Tab::ALL
        .iter()
        .map(|tab| SelectorBarItem::new(tab.name(), tab.name()))
        .collect();
    let selected = Tab::ALL.iter().position(|tab| *tab == shown);
    let inspector = inspector.clone();
    SelectorBar::new(items, selected, move |app, asked| {
        if let Some(tab) = asked.and_then(|index| Tab::ALL.get(index)) {
            show_tab(app, &inspector, *tab);
        }
    })
    .into_widget()
}

/// `text` on one line, cut short where it is too long, primary or secondary.
fn one_line(
    text: &str,
    style: TextBlockStyle,
    resources: &ThemeResources,
    primary: bool,
) -> WidgetRef {
    let color = if primary {
        resources.text_fill_color_primary
    } else {
        resources.text_fill_color_secondary
    };
    Text::new(text.to_owned())
        .style(style.text_style(resources, color))
        .max_lines(1)
        .overflow(TextOverflow::Ellipsis)
        .into_widget()
}

/// `text` in the body size, as many lines as it takes.
fn paragraph(text: &str, resources: &ThemeResources) -> WidgetRef {
    let style = TextBlockStyle::Body.text_style(resources, resources.text_fill_color_primary);
    Padding::new(EdgeInsetsGeometry::only(0.0, 2.0, 0.0, 2.0))
        .child(Text::new(text.to_owned()).style(style))
        .into_widget()
}

/// `text` alone, in the middle of what it fills.
fn said(text: &str, resources: &ThemeResources) -> WidgetRef {
    Center::new()
        .child(Padding::new(EdgeInsetsGeometry::all(MARGIN)).child(
            Text::new(text.to_owned()).style(
                TextBlockStyle::Body.text_style(resources, resources.text_fill_color_secondary),
            ),
        ))
        .into_widget()
}
