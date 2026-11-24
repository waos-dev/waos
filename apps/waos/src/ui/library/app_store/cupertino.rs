//! The app store at a phone's size, in Cupertino's widgets, laid out as an app of its own: a tab
//! bar of its three places along its foot, over the deck's bar, and the place it is on above it,
//! each a page with Back to the home. Discover: a search field of its own, with a token before
//! what is typed for the source shown alone and for the category chosen, which a tap takes
//! back; a chip a category while none is chosen; the apps the sources put forward as a shelf of
//! cards that scrolls sideways; and every app as a row of an inset list. Updates: Update all,
//! and the apps with a newer version. Sources: the field that adds a source, the row that
//! installs from a file where the system has a file picker, then each source, its logo, name
//! and how many apps it lists, which a tap shows alone, over its address and the row that
//! removes it.
//!
//! What the page keeps of its own is the text being typed, a search or a source to add, and
//! the tab its bar is on, which follows the library's place; what it shows is the library's,
//! read through the store's shared rows (`app_store`).

use std::rc::Rc;
use std::sync::Arc;

use waft::ui::FontWeight;
use waft::{
    App, Axis, BorderRadius, BorderRadiusGeometry, BottomNavigationBarItem, BoxDecoration,
    BoxShape, BuildContext, Builder, Clip, Column, CrossAxisAlignment, DecoratedBox,
    EdgeInsetsGeometry, Entity, Expanded, Focus, GestureDetector, Handle, Icon, IntoWidget,
    KeyEventResult, ListView, Listenable, Listener, MainAxisSize, MediaQuery, Padding, Positioned,
    Ref, Row, SingleChildScrollView, SizedBox, Stack, State, StateData, StatefulWidget,
    StatelessWidget, Subscription, Task, Text, TextEditingController, TextEditingControllerLeaf,
    TextOverflow, TextStyle, WidgetRef,
};
use waft_cupertino::{
    CupertinoButton, CupertinoButtonSize, CupertinoColors, CupertinoIcons, CupertinoListSection,
    CupertinoListTile, CupertinoListTileChevron, CupertinoNavigationBar, CupertinoPageScaffold,
    CupertinoSearchTextField, CupertinoTabBar, CupertinoTabController, CupertinoTabScaffold,
    CupertinoTextField, ObstructingPreferredSizeWidgetRef,
};
use waft_rendering::HitTestBehavior;
use waos_package::Category;

use super::{
    Discover, PackagePicker, RowAction, SourceRow, StoreApp, discover, source_name, source_rows,
    updates,
};
use crate::logic::app_store::AppStorePlace;
use crate::state::library::{Library, LibraryPage};
use crate::ui::library::cupertino::back_to_home;
use crate::ui::library::home::cupertino::resolved;
use crate::ui::library::program_icon::ProgramIcon;
use crate::ui::library::search_field::takes_back_a_token;
use crate::ui::library::words;

/// The page's margins, as an inset list's.
const SIDE_MARGIN: f64 = 20.0;

/// The side of an app's icon in a row, in a card of the apps put forward, and of a source's
/// logo.
const ICON_SIDE: f64 = 44.0;
const CARD_ICON_SIDE: f64 = 56.0;
const SOURCE_LOGO_SIDE: f64 = 32.0;

/// How wide a card of the apps put forward is, so the next one shows at the screen's edge, and
/// how round its corners are.
const CARD_WIDTH: f64 = 300.0;
const CARD_CORNER_RADIUS: f64 = 14.0;

/// The store's places, in the order the tab bar shows them.
const PLACES: [AppStorePlace; 3] = [
    AppStorePlace::Discover,
    AppStorePlace::Updates,
    AppStorePlace::Sources,
];

/// The app store, in Cupertino's widgets.
#[derive(Debug)]
pub(crate) struct CupertinoAppStore {
    pub(crate) library: Entity<Library>,
}

impl StatefulWidget for CupertinoAppStore {
    type State = CupertinoAppStoreState;

    fn create_state(&self) -> CupertinoAppStoreState {
        CupertinoAppStoreState {
            state: StateData::new(),
            kept: None,
            followed: None,
        }
    }
}

pub(crate) struct CupertinoAppStoreState {
    state: StateData<CupertinoAppStore>,
    kept: Option<Kept>,
    /// The library's changes, heard while the page shows.
    followed: Option<Subscription>,
}

/// What the page keeps of its own.
#[derive(Clone)]
struct Kept {
    /// What the apps are searched for.
    search: Ref<TextEditingController>,
    /// A source to add.
    source: Ref<TextEditingController>,
    /// The tab the bar is on.
    tabs: Ref<CupertinoTabController>,
}

/// Where `place` is among the bar's tabs.
fn tab_of(place: AppStorePlace) -> usize {
    PLACES
        .iter()
        .position(|shown| *shown == place)
        .expect("every place has a tab")
}

impl State for CupertinoAppStoreState {
    type Widget = CupertinoAppStore;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let library = self.widget(app).library.clone();
        let search = TextEditingController::new(app);
        search.set_text(app, library.read(app).app_store_search().to_owned());
        let (typed_in, searched) = (search.handle(), library.clone());
        search.add_listener(
            app,
            Listener::new(move |app| {
                let typed = typed_in.text_value(app).to_owned();
                searched.update(app, |library, cx| library.search_app_store_for(&typed, cx));
            }),
        );
        let place = library.read(app).app_store_place();
        let kept = Kept {
            search,
            source: TextEditingController::new(app),
            tabs: CupertinoTabController::initial_index(app, tab_of(place)),
        };
        let followed = app.observe(&library, move |app| self.follow_the_place(app));
        let state = app.get_mut(self);
        state.kept = Some(kept);
        state.followed = Some(followed);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        app.get_mut(self).followed = None;
        if let Some(kept) = app.get_mut(self).kept.take() {
            kept.search.dispose(app);
            kept.source.dispose(app);
            kept.tabs.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let library = self.widget(app).library.clone();
        let kept = app.get(self).kept.clone().expect("an initialized page");
        let waiting = library.read(app).updates().len();
        let badge = resolved(CupertinoColors::SYSTEM_RED, app, context);
        let chosen = library.clone();
        let bar = CupertinoTabBar::new(tabs(waiting, badge)).on_tap(move |app, tab| {
            if let Some(place) = PLACES.get(tab) {
                chosen.update(app, |library, cx| library.show_app_store_place(*place, cx));
            }
        });
        let tabs = kept.tabs.clone();
        CupertinoTabScaffold::new(bar, move |_, _, tab| {
            StorePlace {
                library: library.clone(),
                kept: kept.clone(),
                place: PLACES[tab as usize],
            }
            .into_widget()
        })
        .controller(tabs)
        .into_widget()
    }
}

impl CupertinoAppStoreState {
    /// The library changed: the bar goes to the tab of the place the library is on, as when
    /// Show apps of a source takes the store to Discover.
    fn follow_the_place(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        let place = self.widget(app).library.read(app).app_store_place();
        if let Some(kept) = &app.get(self).kept {
            kept.tabs.handle().set_index(app, tab_of(place));
        }
    }
}

/// The bar's tabs, in the order of [`PLACES`]; Updates says how many there are, where there are
/// any, on a badge of `badge_color`.
fn tabs(waiting: usize, badge_color: waft::Color) -> Vec<BottomNavigationBarItem> {
    let updates = Icon::new(Some(CupertinoIcons::arrow_down_circle())).into_widget();
    let updates = if waiting == 0 {
        updates
    } else {
        Stack::new()
            .clip_behavior(Clip::None)
            .children([
                updates,
                Positioned {
                    top: Some(-2.0),
                    right: Some(-10.0),
                    ..Positioned::new(count_badge(waiting, badge_color))
                }
                .into_widget(),
            ])
            .into_widget()
    };
    vec![
        BottomNavigationBarItem::new(Icon::new(Some(CupertinoIcons::compass())))
            .label(words::DISCOVER),
        BottomNavigationBarItem::new(updates).label(words::UPDATES),
        BottomNavigationBarItem::new(Icon::new(Some(CupertinoIcons::globe())))
            .label(words::SOURCES),
    ]
}

/// A count on a tab's icon, as iOS badges one: white on a capsule of `color`.
fn count_badge(count: usize, color: waft::Color) -> WidgetRef {
    let capsule = BoxDecoration {
        color: Some(color.into()),
        border_radius: Some(BorderRadiusGeometry::circular(9.0)),
        ..BoxDecoration::default()
    };
    let said = Text::new(count.to_string()).style(
        TextStyle::new()
            .font_size(12.0)
            .font_weight(FontWeight::W600)
            .color(CupertinoColors::WHITE),
    );
    DecoratedBox::new(capsule)
        .child(Padding::new(EdgeInsetsGeometry::symmetric(1.0, 5.0)).child(said))
        .into_widget()
}

/// One of the store's places, as the page of its tab.
#[derive(Clone)]
struct StorePlace {
    library: Entity<Library>,
    kept: Kept,
    place: AppStorePlace,
}

impl std::fmt::Debug for StorePlace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorePlace")
            .field("place", &self.place)
            .finish_non_exhaustive()
    }
}

impl StatelessWidget for StorePlace {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let page = Page {
            package_picker: PackagePicker::of(&self.library, app, context),
            library: self.library.clone(),
            kept: self.kept.clone(),
            colors: Colors::of(app, context),
        };
        let parts = match self.place {
            AppStorePlace::Discover => page.discover(app),
            AppStorePlace::Updates => page.updates(app),
            AppStorePlace::Sources => page.sources(app),
        };
        let bar = CupertinoNavigationBar::new()
            .leading(back_to_home(&self.library))
            .middle(Text::new(words::GET_PROGRAMS));
        CupertinoPageScaffold::new(between_bars(move |top, bottom| {
            ListView::new()
                .padding(EdgeInsetsGeometry::from_ltrb(0.0, top, 0.0, bottom + 24.0))
                .children(parts.clone())
                .into_widget()
        }))
        .navigation_bar(ObstructingPreferredSizeWidgetRef::new(bar))
        .background_color(CupertinoColors::SYSTEM_GROUPED_BACKGROUND)
        .into_widget()
    }
}

/// What `build` makes of how far the page's navigation bar reaches down and the store's tab bar
/// up: the padding the scaffolds report to what is inside them, so a list under translucent
/// bars starts and ends clear of them and scrolls beneath them.
fn between_bars(build: impl Fn(f64, f64) -> WidgetRef + 'static) -> WidgetRef {
    Builder::new(move |app, context| {
        let padding = MediaQuery::padding_of(app, context);
        build(padding.top, padding.bottom)
    })
    .into_widget()
}

/// The colours a place draws in, as the theme it is built in resolves them.
#[derive(Clone, Copy)]
struct Colors {
    /// What the page says beside its main text.
    secondary: waft::Color,
    /// A row that acts, and one that removes.
    tint: waft::Color,
    destructive: waft::Color,
    /// The ground of a chip and of a token, and of a card.
    fill: waft::Color,
    card: waft::Color,
}

impl Colors {
    fn of(app: &mut App, context: BuildContext) -> Colors {
        let mut resolve = |color| resolved(color, app, context);
        Colors {
            secondary: resolve(CupertinoColors::SECONDARY_LABEL),
            tint: resolve(CupertinoColors::ACTIVE_BLUE),
            destructive: resolve(CupertinoColors::SYSTEM_RED),
            fill: resolve(CupertinoColors::TERTIARY_SYSTEM_FILL),
            card: resolve(CupertinoColors::SECONDARY_SYSTEM_GROUPED_BACKGROUND),
        }
    }
}

/// What one build of a place reads and writes.
struct Page {
    library: Entity<Library>,
    kept: Kept,
    /// What Install from file shows; none where the system has no file picker.
    package_picker: Option<PackagePicker>,
    colors: Colors,
}

impl Page {
    /// The first place: the search field, the categories' chips while none is chosen, then the
    /// apps put forward and every app.
    fn discover(&self, app: &App) -> Vec<WidgetRef> {
        let library = self.library.read(app);
        let Discover {
            featured,
            categories,
            apps,
        } = discover(library);
        let mut parts = vec![self.search_field(library)];
        // A category chosen is a token in the search field; the chips are what chooses one.
        if !categories.is_empty() && library.app_store_category().is_none() {
            parts.push(self.category_chips(&categories));
        }
        // Every app has a heading only where the apps put forward stand over it.
        let all_apps = (!featured.is_empty()).then_some(words::ALL_APPS);
        if !featured.is_empty() {
            parts.push(self.heading(words::FEATURED));
            parts.push(self.shelf(featured));
        }
        let rows: Vec<WidgetRef> = if apps.is_empty() {
            let nothing = if library.catalog().is_empty() {
                words::NO_APPS_LISTED
            } else {
                words::NO_PROGRAMS_FOUND
            };
            vec![note(nothing)]
        } else {
            apps.into_iter().map(|app| self.app_tile(app)).collect()
        };
        parts.push(section(all_apps, rows));
        parts
    }

    /// The second place: Update all, then the apps with a newer version; one line where every
    /// app is up to date.
    fn updates(&self, app: &App) -> Vec<WidgetRef> {
        let waiting = updates(self.library.read(app));
        if waiting.is_empty() {
            return vec![section(None, vec![note(words::UP_TO_DATE)])];
        }
        let library = self.library.clone();
        let update_all = Rc::new(move |app: &mut App| {
            library.update(app, |library, cx| library.update_all(cx));
            Task::ready(())
        });
        let all = CupertinoListTile::new(self.acting(words::UPDATE_ALL, self.colors.tint))
            .on_tap(update_all)
            .into_widget();
        let rows = waiting.into_iter().map(|app| self.app_tile(app)).collect();
        vec![section(None, vec![all]), section(None, rows)]
    }

    /// The third place: what adds a source or installs from a file, then each source.
    fn sources(&self, app: &App) -> Vec<WidgetRef> {
        let mut parts = vec![self.add_source()];
        parts.extend(self.install_from_file());
        let sources = source_rows(self.library.read(app));
        if sources.is_empty() {
            parts.push(section(None, vec![note(words::NO_SOURCES)]));
        }
        parts.extend(sources.into_iter().map(|source| self.source(source)));
        parts
    }

    /// The field the apps are searched in, with a token before what is typed for the source
    /// shown alone and for the category chosen.
    fn search_field(&self, library: &Library) -> WidgetRef {
        let mut before = vec![
            Icon::new(Some(CupertinoIcons::search()))
                .size(20.0)
                .color(self.colors.secondary)
                .into_widget(),
        ];
        if let Some(only) = library.app_store_source() {
            let shown = self.library.clone();
            before.push(self.token(&source_name(only), move |app| {
                shown.update(app, |library, cx| library.show_app_store_source(None, cx));
            }));
        }
        if let Some(category) = library.app_store_category() {
            let shown = self.library.clone();
            before.push(self.token(words::category(category), move |app| {
                shown.update(app, |library, cx| {
                    library.show_app_store_category(None, cx);
                });
            }));
        }
        // A token says what is searched already: no placeholder after it.
        let placeholder = if before.len() == 1 {
            words::SEARCH_STORE
        } else {
            ""
        };
        let field = CupertinoSearchTextField::new()
            .controller(self.kept.search.erase())
            .placeholder(placeholder)
            .prefix_icon(
                Row::new()
                    .main_axis_size(MainAxisSize::Min)
                    .spacing(6.0)
                    .children(before),
            );
        // A keyboard with a Backspace key takes the last token back, as on the desktop; a touch
        // keyboard sends none in an empty field, where a tap on the token does.
        let (library, typed) = (self.library.clone(), self.kept.search.handle());
        let field = Focus::new(field)
            .skip_traversal(true)
            .can_request_focus(false)
            .on_key_event(Rc::new(move |app, _, event| {
                let typed = typed.text_value(app).to_owned();
                if takes_back_a_token(event, &typed)
                    && library.update(app, |library, cx| library.widen_app_store(cx))
                {
                    KeyEventResult::Handled
                } else {
                    KeyEventResult::Ignored
                }
            }));
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            SIDE_MARGIN,
            12.0,
            SIDE_MARGIN,
            8.0,
        ))
        .child(field)
        .into_widget()
    }

    /// The token of what narrows the search to `name`, with the mark that takes it back: a tap
    /// on it does `take_back`.
    fn token(&self, name: &str, take_back: impl Fn(&mut App) + 'static) -> WidgetRef {
        let ground = BoxDecoration {
            color: Some(self.colors.fill.into()),
            border_radius: Some(BorderRadiusGeometry::circular(6.0)),
            ..BoxDecoration::default()
        };
        let said = Row::new()
            .main_axis_size(MainAxisSize::Min)
            .spacing(4.0)
            .children([
                Text::new(name).into_widget(),
                Icon::new(Some(CupertinoIcons::xmark_circle_fill()))
                    .size(14.0)
                    .color(self.colors.secondary)
                    .into_widget(),
            ]);
        GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(Listener::new(take_back))
            .child(
                DecoratedBox::new(ground)
                    .child(Padding::new(EdgeInsetsGeometry::symmetric(2.0, 6.0)).child(said)),
            )
            .into_widget()
    }

    /// A chip for each of `categories`, in a row that scrolls sideways where the chips outgrow
    /// it; a tap on one shows its category alone.
    fn category_chips(&self, categories: &[Category]) -> WidgetRef {
        let chips: Vec<WidgetRef> = categories
            .iter()
            .map(|&category| {
                let library = self.library.clone();
                let choose = Listener::new(move |app| {
                    library.update(app, |library, cx| {
                        library.show_app_store_category(Some(category), cx);
                    });
                });
                CupertinoButton::new(
                    Text::new(words::category(category)).into_widget(),
                    Some(choose),
                )
                .size_style(CupertinoButtonSize::Small)
                .color(self.colors.fill)
                .foreground_color(CupertinoColors::LABEL)
                .border_radius(BorderRadius::circular(100.0))
                .into_widget()
            })
            .collect();
        SingleChildScrollView::new()
            .scroll_direction(Axis::Horizontal)
            .padding(EdgeInsetsGeometry::from_ltrb(
                SIDE_MARGIN,
                0.0,
                SIDE_MARGIN,
                4.0,
            ))
            .child(Row::new().spacing(8.0).children(chips))
            .into_widget()
    }

    /// A heading over a part that is no inset list, set as an inset list's own heading is.
    fn heading(&self, text: &str) -> WidgetRef {
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            SIDE_MARGIN,
            16.0,
            SIDE_MARGIN,
            6.0,
        ))
        .child(
            Text::new(text).style(
                TextStyle::new()
                    .font_size(20.0)
                    .font_weight(FontWeight::W700),
            ),
        )
        .into_widget()
    }

    /// The apps put forward, as cards side by side on a shelf that scrolls sideways.
    fn shelf(&self, featured: Vec<StoreApp>) -> WidgetRef {
        let cards: Vec<WidgetRef> = featured.into_iter().map(|app| self.card(app)).collect();
        SingleChildScrollView::new()
            .scroll_direction(Axis::Horizontal)
            .padding(EdgeInsetsGeometry::symmetric(0.0, SIDE_MARGIN))
            .child(Row::new().spacing(12.0).children(cards))
            .into_widget()
    }

    /// An app put forward: its icon, name and line, and what can be done with it, on a card a
    /// tap on which opens the app's page.
    fn card(&self, app: StoreApp) -> WidgetRef {
        let ground = BoxDecoration {
            color: Some(self.colors.card.into()),
            border_radius: Some(BorderRadiusGeometry::circular(CARD_CORNER_RADIUS)),
            ..BoxDecoration::default()
        };
        let icon = ProgramIcon {
            png: app.icon.clone(),
            side: CARD_ICON_SIDE,
            badge: None,
        };
        let name = Text::new(app.app.listed.name.as_str())
            .max_lines(1)
            .overflow(TextOverflow::Ellipsis)
            .style(TextStyle::new().font_weight(FontWeight::W600));
        let line = Text::new(app.line.as_str())
            .max_lines(2)
            .overflow(TextOverflow::Ellipsis)
            .style(
                TextStyle::new()
                    .font_size(13.0)
                    .color(self.colors.secondary),
            );
        let text = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .children([name.into_widget(), line.into_widget()]);
        let said = Row::new().spacing(12.0).children([
            icon.into_widget(),
            Expanded::new(text).into_widget(),
            self.action(&app),
        ]);
        GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(self.opens_the_page_of(&app))
            .child(
                SizedBox::new().width(CARD_WIDTH).child(
                    DecoratedBox::new(ground)
                        .child(Padding::new(EdgeInsetsGeometry::all(12.0)).child(said)),
                ),
            )
            .into_widget()
    }

    /// An app of the store, as a row of an inset list: its icon, name and line, and what can be
    /// done with it; a tap on the row opens the app's page.
    fn app_tile(&self, app: StoreApp) -> WidgetRef {
        let icon = ProgramIcon {
            png: app.icon.clone(),
            side: ICON_SIDE,
            badge: None,
        };
        let open = self.opens_the_page_of(&app);
        let open_page = Rc::new(move |app: &mut App| {
            open.call(app);
            Task::ready(())
        });
        CupertinoListTile::new(Text::new(app.app.listed.name.clone()))
            .subtitle(Text::new(app.line.clone()))
            .leading(icon)
            .leading_size(ICON_SIDE)
            .trailing(self.action(&app))
            .on_tap(open_page)
            .into_widget()
    }

    /// What shows the page of `app`.
    fn opens_the_page_of(&self, app: &StoreApp) -> Listener {
        let (library, id) = (self.library.clone(), app.app.listed.id.clone());
        Listener::new(move |app| {
            let page = LibraryPage::App(id.clone());
            library.update(app, |library, cx| library.show_page(page, cx));
        })
    }

    /// What can be done with `app`: Get or Update as a button; Installing… or Installed as a
    /// word.
    fn action(&self, app: &StoreApp) -> WidgetRef {
        let label = match app.action {
            RowAction::Installing => return self.said(words::INSTALLING),
            RowAction::Installed => return self.said(words::INSTALLED),
            RowAction::Get => words::GET,
            RowAction::Update => words::UPDATE,
        };
        let (library, listed) = (self.library.clone(), app.app.clone());
        let install = Listener::new(move |app| {
            library.update(app, |library, cx| {
                library.install_from(&listed.source, &listed.listed, cx);
            });
        });
        CupertinoButton::tinted(Text::new(label).into_widget(), Some(install))
            .size_style(CupertinoButtonSize::Small)
            .into_widget()
    }

    /// The field a source is typed in, and Add.
    fn add_source(&self) -> WidgetRef {
        let (library, text) = (self.library.clone(), self.kept.source.clone());
        let add = Listener::new(move |app| {
            let typed = text.text_value(app).to_owned();
            library.update(app, |library, cx| library.add_source(&typed, cx));
            if library.read(app).problem().is_none() {
                text.clear(app);
            }
        });
        let field = CupertinoTextField::new()
            .controller(self.kept.source.erase())
            .placeholder(words::SOURCE_PLACEHOLDER);
        let tile = CupertinoListTile::new(field)
            .trailing(CupertinoButton::new(
                Text::new(words::ADD).into_widget(),
                Some(add),
            ))
            .into_widget();
        section(Some(words::ADD_SOURCE), vec![tile])
    }

    /// The row that shows the system's file picker for a package, in the tint colour, in a
    /// section of its own; none where the system has no file picker.
    fn install_from_file(&self) -> Option<WidgetRef> {
        let picker = self.package_picker.clone()?;
        let pick = Rc::new(move |app: &mut App| {
            picker.show(app);
            Task::ready(())
        });
        let row = self.acting(words::INSTALL_FROM_FILE_ROW, self.colors.tint);
        let tile = CupertinoListTile::new(row).on_tap(pick).into_widget();
        Some(section(None, vec![tile]))
    }

    /// A source, as a list of its own: its logo, what it calls itself and how many apps it
    /// lists, or what it says, a tap on which shows its apps alone; its address; and the row
    /// that removes it.
    fn source(&self, row: SourceRow) -> WidgetRef {
        let said = match &row.lists {
            Ok(apps) => words::apps_listed(*apps),
            Err(said) => said.clone(),
        };
        let named = CupertinoListTile::new(Text::new(row.name.clone()))
            .subtitle(Text::new(said))
            .leading(self.source_logo(row.logo.clone()))
            .leading_size(SOURCE_LOGO_SIDE);
        let named = if row.lists.as_ref().is_ok_and(|apps| *apps > 0) {
            let (library, shown) = (self.library.clone(), row.source.clone());
            let show = Rc::new(move |app: &mut App| {
                library.update(app, |library, cx| {
                    library.show_app_store_source(Some(shown.clone()), cx);
                });
                Task::ready(())
            });
            named.trailing(CupertinoListTileChevron::new()).on_tap(show)
        } else {
            named
        };
        let address = CupertinoListTile::new(Text::new(words::SOURCE_ADDRESS))
            .additional_info(Text::new(row.address.clone()));
        let (library, removed) = (self.library.clone(), row.source);
        let remove = Rc::new(move |app: &mut App| {
            library.update(app, |library, cx| library.remove_source(&removed, cx));
            Task::ready(())
        });
        let remove =
            CupertinoListTile::new(self.acting(words::REMOVE_SOURCE, self.colors.destructive))
                .on_tap(remove);
        section(
            None,
            vec![
                named.into_widget(),
                address.into_widget(),
                remove.into_widget(),
            ],
        )
    }

    /// A source's logo, or a globe on a faint disc for a source that has none.
    fn source_logo(&self, logo: Option<Arc<[u8]>>) -> WidgetRef {
        if logo.is_some() {
            return ProgramIcon {
                png: logo,
                side: SOURCE_LOGO_SIDE,
                badge: None,
            }
            .into_widget();
        }
        let disc = BoxDecoration {
            color: Some(self.colors.fill.into()),
            shape: BoxShape::Circle,
            ..BoxDecoration::default()
        };
        let globe = Icon::new(Some(CupertinoIcons::globe()))
            .size(SOURCE_LOGO_SIDE * 0.6)
            .color(self.colors.secondary);
        SizedBox::new()
            .width(SOURCE_LOGO_SIDE)
            .height(SOURCE_LOGO_SIDE)
            .child(DecoratedBox::new(disc).child(waft::Center::new().child(globe)))
            .into_widget()
    }

    /// The words of a row that acts, in `color`.
    fn acting(&self, text: &str, color: waft::Color) -> WidgetRef {
        Text::new(text)
            .style(TextStyle::new().color(color))
            .into_widget()
    }

    /// `text` in the secondary colour, where a row says what it would otherwise offer.
    fn said(&self, text: &str) -> WidgetRef {
        Text::new(text)
            .style(TextStyle::new().color(self.colors.secondary))
            .into_widget()
    }
}

/// An inset-grouped section, under `title` where given.
fn section(title: Option<&str>, tiles: Vec<WidgetRef>) -> WidgetRef {
    let section = CupertinoListSection::inset_grouped().children(tiles);
    match title {
        Some(title) => section.header(Text::new(title)),
        None => section,
    }
    .into_widget()
}

/// A tile that only says `text`.
fn note(text: &str) -> WidgetRef {
    CupertinoListTile::new(Text::new(text)).into_widget()
}
