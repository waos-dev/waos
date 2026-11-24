//! The app store in the desktop's panel, in Fluent's controls: Back and the page's title, a
//! selector bar of its three places, and the place it is on. Discover: a field that searches the
//! apps, the source shown alone where one is, a chip a category, the apps the sources put
//! forward as cards, and every app as a row, its icon, name and line, and Get or Update as a
//! button, or Installing… or Installed as a caption. Updates: the apps with a newer version, and
//! Update all. Sources: the field that adds a source with Add and, where the system has a file
//! picker, Install from file… beside it, then each source with how many apps it lists, or what
//! it says while it is read or could not be, Show apps and Remove.
//!
//! What the page keeps of its own is the text being typed, a search or a source to add; what it
//! shows is the library's, read through the store's shared rows (`app_store`).

use std::rc::Rc;

use waft::{
    App, BorderRadiusGeometry, BoxDecoration, BuildContext, Column, CrossAxisAlignment,
    DecoratedBox, EdgeInsetsGeometry, Entity, Expanded, Handle, IntoWidget, ListView, Listenable,
    Listener, MainAxisSize, Padding, Ref, Row, SizedBox, State, StateData, StatefulWidget, Text,
    TextEditingController, TextEditingControllerLeaf, TextOverflow, WidgetRef,
};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FontIcon, InfoBadge, SelectorBar, SelectorBarItem,
    TextBlockStyle, TextBox, ThemeResources,
};

use super::{
    Discover, PackagePicker, RowAction, SourceRow, StoreApp, discover, source_row, source_rows,
    updates,
};
use crate::logic::app_store::AppStorePlace;
use crate::state::library::{Library, LibraryPage};
use crate::ui::library::category_chips::category_chips;
use crate::ui::library::home::fluent::MARGIN;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::words;

const GAP: f64 = 8.0;

/// The side of an app's icon in a row, and in a card of the apps put forward.
const ICON_SIDE: f64 = 40.0;
const CARD_ICON_SIDE: f64 = 48.0;

/// How many cards of the apps put forward stand side by side.
const CARDS_ACROSS: usize = 2;

/// How round a card's corners are: WinUI's overlay corners.
const CARD_CORNER_RADIUS: f64 = 8.0;

/// The store's places, in the order the selector bar shows them.
const PLACES: [AppStorePlace; 3] = [
    AppStorePlace::Discover,
    AppStorePlace::Updates,
    AppStorePlace::Sources,
];

/// The app store, in Fluent's controls.
#[derive(Debug)]
pub(crate) struct FluentAppStore {
    pub(crate) library: Entity<Library>,
}

impl StatefulWidget for FluentAppStore {
    type State = FluentAppStoreState;

    fn create_state(&self) -> FluentAppStoreState {
        FluentAppStoreState {
            state: StateData::new(),
            typed: None,
        }
    }
}

pub(crate) struct FluentAppStoreState {
    state: StateData<FluentAppStore>,
    typed: Option<Typed>,
}

/// What is being typed on the page.
#[derive(Clone)]
struct Typed {
    /// What the apps are searched for.
    search: Ref<TextEditingController>,
    /// A source to add.
    source: Ref<TextEditingController>,
}

impl State for FluentAppStoreState {
    type Widget = FluentAppStore;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let library = self.widget(app).library.clone();
        let search = TextEditingController::new(app);
        search.set_text(app, library.read(app).app_store_search().to_owned());
        let typed_in = search.handle();
        search.add_listener(
            app,
            Listener::new(move |app| {
                let typed = typed_in.text_value(app).to_owned();
                library.update(app, |library, cx| library.search_app_store_for(&typed, cx));
            }),
        );
        let typed = Typed {
            search,
            source: TextEditingController::new(app),
        };
        app.get_mut(self).typed = Some(typed);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(typed) = app.get_mut(self).typed.take() {
            typed.search.dispose(app);
            typed.source.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let library = self.widget(app).library.clone();
        let page = Page {
            package_picker: PackagePicker::of(&library, app, context),
            library,
            typed: app.get(self).typed.clone().expect("an initialized page"),
            resources: ThemeResources::of(app, context),
        };
        let place = page.library.read(app).app_store_place();
        let shown = match place {
            AppStorePlace::Discover => page.discover(app),
            AppStorePlace::Updates => page.updates(app),
            AppStorePlace::Sources => page.sources(app),
        };
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                page.title_bar(),
                page.places(app, place),
                Expanded::new(shown).into_widget(),
            ])
            .into_widget()
    }
}

/// What one build of the page reads and writes.
struct Page {
    library: Entity<Library>,
    typed: Typed,
    /// What Install from file… shows; none where the system has no file picker.
    package_picker: Option<PackagePicker>,
    resources: Rc<ThemeResources>,
}

impl Page {
    /// Back to the home, and the page's title.
    fn title_bar(&self) -> WidgetRef {
        let library = self.library.clone();
        let back = Listener::new(move |app| {
            library.update(app, |library, cx| library.show_page(LibraryPage::Home, cx));
        });
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            MARGIN - 8.0,
            16.0,
            MARGIN,
            4.0,
        ))
        .child(
            Row::new().spacing(4.0).children([
                Button::new(FontIcon::symbol(FluentSymbol::Back).font_size(16.0), back)
                    .style(ButtonStyle::Subtle)
                    .padding([8.0, 8.0, 8.0, 8.0])
                    .into_widget(),
                self.text(words::GET_PROGRAMS, TextBlockStyle::Subtitle),
            ]),
        )
        .into_widget()
    }

    /// The selector bar of the store's places, `place` the one shown; Updates says how many
    /// there are, where there are any.
    fn places(&self, app: &App, place: AppStorePlace) -> WidgetRef {
        let waiting = self.library.read(app).updates().len();
        let items = PLACES
            .iter()
            .map(|place| match place {
                AppStorePlace::Discover => SelectorBarItem::new("discover", words::DISCOVER),
                AppStorePlace::Sources => SelectorBarItem::new("sources", words::SOURCES),
                AppStorePlace::Updates if waiting == 0 => {
                    SelectorBarItem::new("updates", words::UPDATES)
                }
                AppStorePlace::Updates => SelectorBarItem::with_child(
                    "updates",
                    Row::new()
                        .main_axis_size(MainAxisSize::Min)
                        .spacing(6.0)
                        .children([
                            Text::new(words::UPDATES).into_widget(),
                            InfoBadge::new().value(waiting as i32).into_widget(),
                        ]),
                ),
            })
            .collect();
        let selected = PLACES.iter().position(|shown| *shown == place);
        let library = self.library.clone();
        let bar = SelectorBar::new(items, selected, move |app, chosen| {
            if let Some(place) = chosen.and_then(|index| PLACES.get(index)) {
                library.update(app, |library, cx| library.show_app_store_place(*place, cx));
            }
        });
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            MARGIN - 12.0,
            0.0,
            MARGIN,
            8.0,
        ))
        .child(Row::new().children([bar.into_widget()]))
        .into_widget()
    }

    /// The first place: the search field, the source shown alone where one is, the categories'
    /// chips, then the apps put forward and every app.
    fn discover(&self, app: &App) -> WidgetRef {
        let library = self.library.read(app);
        let Discover {
            featured,
            categories,
            apps,
        } = discover(library);
        let mut over = vec![self.search_row()];
        if let Some(only) = library.app_store_source() {
            over.push(self.only_source(&source_row(only)));
        }
        if !categories.is_empty() {
            let chosen = self.library.clone();
            over.push(category_chips(
                &categories,
                library.app_store_category(),
                MARGIN,
                move |app, category| {
                    chosen.update(app, |library, cx| {
                        library.show_app_store_category(category, cx);
                    });
                },
            ));
        }
        let mut rows = Vec::new();
        if !featured.is_empty() {
            rows.push(self.heading(words::FEATURED));
            rows.extend(self.cards(featured));
            rows.push(self.heading(words::ALL_APPS));
        }
        if apps.is_empty() {
            let nothing = if library.catalog().is_empty() {
                words::NO_APPS_LISTED
            } else {
                words::NO_PROGRAMS_FOUND
            };
            rows.push(self.caption(nothing));
        }
        rows.extend(apps.into_iter().map(|app| self.app_row(app)));
        self.under(over, rows)
    }

    /// The second place: the apps with a newer version, under Update all; one line where every
    /// app is up to date.
    fn updates(&self, app: &App) -> WidgetRef {
        let waiting = updates(self.library.read(app));
        if waiting.is_empty() {
            return self.under(Vec::new(), vec![self.caption(words::UP_TO_DATE)]);
        }
        let library = self.library.clone();
        let update_all = Listener::new(move |app| {
            library.update(app, |library, cx| library.update_all(cx));
        });
        let over = Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, 8.0)).child(
            Row::new().children([
                Expanded::new(SizedBox::shrink()).into_widget(),
                Button::text(words::UPDATE_ALL, update_all)
                    .style(ButtonStyle::Accent)
                    .into_widget(),
            ]),
        );
        let rows = waiting.into_iter().map(|app| self.app_row(app)).collect();
        self.under(vec![over.into_widget()], rows)
    }

    /// The third place: what adds a source or installs from a file, then each source.
    fn sources(&self, app: &App) -> WidgetRef {
        let mut rows: Vec<WidgetRef> = source_rows(self.library.read(app))
            .into_iter()
            .map(|source| self.source(source))
            .collect();
        if rows.is_empty() {
            rows.push(self.caption(words::NO_SOURCES));
        }
        self.under(vec![self.add_row()], rows)
    }

    /// `over`, which stays, above `rows`, which scroll.
    fn under(&self, over: Vec<WidgetRef>, rows: Vec<WidgetRef>) -> WidgetRef {
        let list = ListView::new()
            .padding(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, MARGIN))
            .children(rows);
        let mut children = over;
        children.push(Expanded::new(list).into_widget());
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children(children)
            .into_widget()
    }

    /// The field the apps are searched in, which takes the keyboard as it shows.
    fn search_row(&self) -> WidgetRef {
        // The field has the keyboard as Discover shows, so typing searches at once, as the
        // home's does.
        let field = TextBox::new(self.typed.search.erase())
            .placeholder_text(words::SEARCH_STORE)
            .autofocus(true);
        Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, 12.0))
            .child(field)
            .into_widget()
    }

    /// The source whose apps alone show, and what shows every source's again.
    fn only_source(&self, only: &SourceRow) -> WidgetRef {
        let library = self.library.clone();
        let every = Listener::new(move |app| {
            library.update(app, |library, cx| library.show_app_store_source(None, cx));
        });
        Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, 12.0))
            .child(
                Row::new().spacing(GAP).children([
                    Expanded::new(self.text(&only.name, TextBlockStyle::BodyStrong)).into_widget(),
                    Button::text(words::EVERY_SOURCE, every)
                        .style(ButtonStyle::Subtle)
                        .into_widget(),
                ]),
            )
            .into_widget()
    }

    /// The apps put forward as cards, [`CARDS_ACROSS`] side by side.
    fn cards(&self, featured: Vec<StoreApp>) -> Vec<WidgetRef> {
        featured
            .chunks(CARDS_ACROSS)
            .map(|side_by_side| {
                let mut cards: Vec<WidgetRef> = side_by_side
                    .iter()
                    .map(|app| Expanded::new(self.card(app.clone())).into_widget())
                    .collect();
                // A row short of a card keeps its cards the width of the others.
                cards.resize_with(CARDS_ACROSS, || {
                    Expanded::new(SizedBox::shrink()).into_widget()
                });
                Padding::new(EdgeInsetsGeometry::only(0.0, 0.0, 0.0, GAP))
                    .child(Row::new().spacing(GAP).children(cards))
                    .into_widget()
            })
            .collect()
    }

    /// An app put forward: its icon, name and line, and what can be done with it, on a card.
    fn card(&self, app: StoreApp) -> WidgetRef {
        let resources = &self.resources;
        let surface = BoxDecoration {
            color: Some(resources.card_background_fill_color_default.into()),
            border: Some(
                waft::Border::all(
                    resources.card_stroke_color_default,
                    1.0,
                    waft::BorderStyle::Solid,
                    waft::BorderSide::STROKE_ALIGN_INSIDE,
                )
                .into(),
            ),
            border_radius: Some(BorderRadiusGeometry::circular(CARD_CORNER_RADIUS)),
            ..BoxDecoration::default()
        };
        DecoratedBox::new(surface)
            .child(
                Padding::new(EdgeInsetsGeometry::all(12.0))
                    .child(self.app_line(app, CARD_ICON_SIDE)),
            )
            .into_widget()
    }

    /// An app of the store: its icon, name and line, and what can be done with it.
    fn app_row(&self, app: StoreApp) -> WidgetRef {
        Padding::new(EdgeInsetsGeometry::symmetric(6.0, 0.0))
            .child(self.app_line(app, ICON_SIDE))
            .into_widget()
    }

    /// `app`'s icon, `side` across, its name over its line, and its action.
    fn app_line(&self, app: StoreApp, side: f64) -> WidgetRef {
        let icon = ProgramIcon {
            png: app.icon.clone(),
            side,
            badge: None,
        };
        let text = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                self.text(&app.app.listed.name, TextBlockStyle::Body),
                self.caption(&app.line),
            ]);
        Row::new()
            .spacing(12.0)
            .children([
                icon.into_widget(),
                Expanded::new(text).into_widget(),
                self.action(&app),
            ])
            .into_widget()
    }

    /// What can be done with `app`: Get or Update as a button; Installing… or Installed as a
    /// caption.
    fn action(&self, app: &StoreApp) -> WidgetRef {
        let label = match app.action {
            RowAction::Installing => return self.caption(words::INSTALLING),
            RowAction::Installed => return self.caption(words::INSTALLED),
            RowAction::Get => words::GET,
            RowAction::Update => words::UPDATE,
        };
        let (library, app) = (self.library.clone(), app.app.clone());
        let install = Listener::new(move |ui| {
            library.update(ui, |library, cx| {
                library.install_from(&app.source, &app.listed, cx);
            });
        });
        Button::text(label, install)
            .style(ButtonStyle::Accent)
            .into_widget()
    }

    /// The field a source is typed in, Add, and Install from file… where the system has a file
    /// picker.
    fn add_row(&self) -> WidgetRef {
        let (library, text) = (self.library.clone(), self.typed.source.clone());
        let add = Listener::new(move |app| {
            let typed = text.text_value(app).to_owned();
            library.update(app, |library, cx| library.add_source(&typed, cx));
            if library.read(app).problem().is_none() {
                text.clear(app);
            }
        });
        let field =
            TextBox::new(self.typed.source.erase()).placeholder_text(words::SOURCE_PLACEHOLDER);
        let mut controls = vec![
            Expanded::new(field).into_widget(),
            Button::text(words::ADD, add).into_widget(),
        ];
        controls.extend(self.install_from_file());
        Padding::new(EdgeInsetsGeometry::from_ltrb(MARGIN, 0.0, MARGIN, 12.0))
            .child(Row::new().spacing(GAP).children(controls))
            .into_widget()
    }

    /// Install from file…, which shows the system's file picker for a package; none where the
    /// system has no file picker.
    fn install_from_file(&self) -> Option<WidgetRef> {
        let picker = self.package_picker.clone()?;
        let pick = Listener::new(move |app| picker.show(app));
        Some(Button::text(words::INSTALL_FROM_FILE, pick).into_widget())
    }

    /// A source: what it calls itself over its address and how many apps it lists, or what it
    /// says; Show apps, where it lists any, and Remove.
    fn source(&self, row: SourceRow) -> WidgetRef {
        let said = match &row.lists {
            Ok(apps) => words::apps_listed(*apps),
            Err(said) => said.clone(),
        };
        let under_name = match &row.address {
            Some(address) => format!("{address} · {said}"),
            None => said,
        };
        let text = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                self.text(&row.name, TextBlockStyle::BodyStrong),
                self.caption(&under_name),
            ]);
        let mut controls = vec![Expanded::new(text).into_widget()];
        if row.lists.as_ref().is_ok_and(|apps| *apps > 0) {
            let (library, shown) = (self.library.clone(), row.source.clone());
            let show = Listener::new(move |app| {
                library.update(app, |library, cx| {
                    library.show_app_store_source(Some(shown.clone()), cx);
                });
            });
            controls.push(Button::text(words::SHOW_APPS, show).into_widget());
        }
        let (library, removed) = (self.library.clone(), row.source);
        let remove = Listener::new(move |app| {
            library.update(app, |library, cx| library.remove_source(&removed, cx));
        });
        controls.push(
            Button::text(words::REMOVE_SOURCE, remove)
                .style(ButtonStyle::Subtle)
                .into_widget(),
        );
        Padding::new(EdgeInsetsGeometry::symmetric(8.0, 0.0))
            .child(Row::new().spacing(GAP).children(controls))
            .into_widget()
    }

    /// A heading over a group of rows.
    fn heading(&self, text: &str) -> WidgetRef {
        Padding::new(EdgeInsetsGeometry::only(0.0, 4.0, 0.0, 8.0))
            .child(self.text(text, TextBlockStyle::BodyStrong))
            .into_widget()
    }

    /// `text` on one line, in `style` and the primary colour.
    fn text(&self, text: &str, style: TextBlockStyle) -> WidgetRef {
        let resources = &self.resources;
        Text::new(text)
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis)
            .style(style.text_style(resources, resources.text_fill_color_primary))
            .into_widget()
    }

    fn caption(&self, text: &str) -> WidgetRef {
        let resources = &self.resources;
        Text::new(text)
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis)
            .style(
                TextBlockStyle::Caption.text_style(resources, resources.text_fill_color_secondary),
            )
            .into_widget()
    }
}
