//! The app at a desktop's or a tablet's width, in Fluent's controls: the programs in a selectable
//! list, WinUI's `ItemsView` of `ItemContainer`s, each its icon, name, what is said of it and what
//! it used; beside it, what the program the user selected did over the last seven days, a section
//! per capability.
//!
//! The invariant: the user's selection chooses the program shown beside the list, and the list
//! selects again the program chosen as the programs are read again, at its new place.

use std::rc::Rc;

use waft::{
    App, BuildContext, Center, ColoredBox, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity,
    Expanded, Handle, IntoWidget, ListView, MainAxisSize, Padding, PageRouteBuilder, Ref,
    RouteSettingsRef, Row, SizedBox, State, StateData, StatefulWidget, StatelessWidget, Text,
    TextOverflow, WidgetRef, WidgetsApp,
};
use waft_winui::{
    AccentPalette, ItemContainer, ItemsView, ItemsViewSelection, ItemsViewSelectionMode,
    ProgressRing, TextBlockStyle, ThemeResources,
};

use crate::inspector::{Found, Inspector, Seen, want_icon};
use crate::program_icon::ProgramIcon;
use crate::words;

const MARGIN: f64 = 24.0;
const GAP: f64 = 12.0;
const LIST_WIDTH: f64 = 320.0;
const ROW_ICON: f64 = 32.0;
const HEADER_ICON: f64 = 48.0;

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
                    Expanded::new(ActivityPane {
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

/// The programs, a selectable list.
#[derive(Debug)]
struct ProgramList {
    inspector: Entity<Inspector>,
}

impl StatefulWidget for ProgramList {
    type State = ProgramListState;

    fn create_state(&self) -> ProgramListState {
        ProgramListState {
            state: StateData::new(),
            selection: None,
        }
    }
}

struct ProgramListState {
    state: StateData<ProgramList>,
    /// The list's selection, which follows the program chosen.
    selection: Option<Ref<ItemsViewSelection>>,
}

impl ProgramListState {
    fn selection(self: Handle<Self>, app: &App) -> Handle<ItemsViewSelection> {
        app.get(self)
            .selection
            .as_ref()
            .map(Ref::handle)
            .expect("made in `init_state`")
    }

    /// The list selects the program chosen, at its place now.
    fn follow_the_choice(self: Handle<Self>, app: &mut App) {
        let inspector = self.widget(app).inspector.clone();
        let read = inspector.read(app);
        let chosen = read.chosen.as_deref().and_then(|chosen| {
            read.programs
                .iter()
                .position(|seen| seen.program.id == chosen)
        });
        let selection = self.selection(app);
        match chosen {
            Some(index) => selection.select(app, index),
            None => selection.deselect_all(app),
        }
    }

    /// The user selected a program: it is chosen.
    fn selected(self: Handle<Self>, app: &mut App) {
        let Some(index) = self.selection(app).selected_index(app) else {
            return;
        };
        let inspector = self.widget(app).inspector.clone();
        let id = inspector
            .read(app)
            .programs
            .get(index)
            .map(|seen| seen.program.id.clone());
        let unchanged = id.is_none() || inspector.read(app).chosen == id;
        if !unchanged {
            inspector.update(app, |inspector, cx| inspector.choose(id, cx));
        }
    }
}

impl State for ProgramListState {
    type Widget = ProgramList;
    waft::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let selection = ItemsViewSelection::new(app);
        app.get_mut(self).selection = Some(selection);
        self.follow_the_choice(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, _old: &ProgramList) {
        self.follow_the_choice(app);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        self.selection(app).dispose(app);
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let inspector = self.widget(app).inspector.clone();
        let programs = inspector.read(app).programs.clone();
        if programs.is_empty() {
            return said(words::NO_PROGRAMS, &resources);
        }
        let selection = app
            .get(self)
            .selection
            .clone()
            .expect("made in `init_state`");
        let rows = Rc::new(programs);
        let shown = Rc::clone(&rows);
        let list = ItemsView::new(rows.len(), move |app, context, index| {
            let resources = ThemeResources::of(app, context);
            ItemContainer::new(program_row(app, &inspector, &shown[index], &resources))
        })
        .selection_mode(ItemsViewSelectionMode::Single)
        .selection(selection)
        .selection_changed(move |app| self.selected(app))
        .padding([GAP, 0.0, GAP, GAP]);
        let title = Text::new(words::PROGRAMS).style(
            TextBlockStyle::Subtitle.text_style(&resources, resources.text_fill_color_primary),
        );
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Padding::new(EdgeInsetsGeometry::from_ltrb(GAP, MARGIN, GAP, 0.0))
                    .child(title)
                    .into_widget(),
                Expanded::new(list).into_widget(),
            ])
            .into_widget()
    }
}

/// A program's row: its icon, its name, what is said of it, and what it used. Its icon is asked
/// for as the row is first shown.
fn program_row(
    app: &mut App,
    inspector: &Entity<Inspector>,
    seen: &Seen,
    resources: &ThemeResources,
) -> WidgetRef {
    want_icon(app, inspector, &seen.program.id);
    let icon = ProgramIcon {
        png: inspector.read(app).icon(&seen.program.id),
        side: ROW_ICON,
    };
    let mut lines = vec![
        one_line(&seen.program.name, TextBlockStyle::Body, resources, true),
        one_line(
            &words::detail(seen),
            TextBlockStyle::Caption,
            resources,
            false,
        ),
    ];
    let marks = words::marks(seen);
    if !marks.is_empty() {
        lines.push(one_line(&marks, TextBlockStyle::Caption, resources, false));
    }
    let text = Column::new()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .children(lines);
    Padding::new(EdgeInsetsGeometry::symmetric(GAP, 8.0))
        .child(
            Row::new()
                .spacing(GAP)
                .children([icon.into_widget(), Expanded::new(text).into_widget()]),
        )
        .into_widget()
}

/// What the program chosen did over the last seven days.
#[derive(Debug)]
struct ActivityPane {
    inspector: Entity<Inspector>,
}

impl StatelessWidget for ActivityPane {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let Some(seen) = self.inspector.read(app).chosen().cloned() else {
            return said(words::CHOOSE, &resources);
        };
        let primary = resources.text_fill_color_primary;
        let mut rows = vec![self.header(app, &seen, &resources)];
        let sections = seen.activity.sections();
        if sections.is_empty() {
            rows.push(paragraph(words::USED_NOTHING, &resources));
        }
        for section in sections {
            let title = Text::new(section.title)
                .style(TextBlockStyle::BodyStrong.text_style(&resources, primary));
            rows.push(
                Padding::new(EdgeInsetsGeometry::only(0.0, MARGIN, 0.0, 4.0))
                    .child(title)
                    .into_widget(),
            );
            rows.extend(section.lines.iter().map(|line| paragraph(line, &resources)));
        }
        ListView::new()
            .padding(EdgeInsetsGeometry::all(MARGIN))
            .children(rows)
            .into_widget()
    }
}

impl ActivityPane {
    /// The program's icon and name, with what is said of it and of the days shown.
    fn header(&self, app: &mut App, seen: &Seen, resources: &ThemeResources) -> WidgetRef {
        let icon = ProgramIcon {
            png: self.inspector.read(app).icon(&seen.program.id),
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
                one_line(words::LAST_DAYS, TextBlockStyle::Caption, resources, false),
            ]);
        Row::new()
            .spacing(GAP)
            .children([icon.into_widget(), Expanded::new(text).into_widget()])
            .into_widget()
    }
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
