//! The launcher's home: a search row as its top, over the home's body (`FluentHomeBody`), the
//! grid of the programs installed with who records above it. The row is a magnifier, the field
//! the user types in, which has the keyboard whenever the launcher shows, and "…", whose menu
//! opens the rest of the library in the same card: getting programs and every program's settings.
//! What is typed is what the library searches its programs for, so the grid filters as the user
//! types, as the phone's home filters by its search pill; Enter opens the first program shown. A
//! category chosen shows in the field as a token before what is typed, which Backspace takes
//! back (`ui::library::search_field`).
//!
//! The invariant: the field holds what the library searches for. What the user types goes to the
//! library, and the library's search changed from elsewhere, as the launcher goes and empties it,
//! is put in the field.

use std::rc::Rc;
use std::time::Duration;

use waft::{
    App, BuildContext, Builder, Column, CrossAxisAlignment, Curves, EdgeInsetsGeometry, Entity,
    Expanded, Focus, FocusNode, FocusNodeLeaf, FrameCallback, GlobalKey, Handle, IntoWidget,
    KeyEventResult, Listenable, Listener, MediaQuery, OptionRetained, Padding, Ref, Row,
    SchedulerBinding, ScrollPositionAlignmentPolicy, Scrollable, State, StateData, StatefulWidget,
    Subscription, TextEditingController, TextEditingControllerLeaf, WidgetRef,
};
use waft_services::{KeyEvent, LogicalKeyboardKey};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FlyoutTarget, FontIcon, MenuFlyout, MenuFlyoutItem, TextBox,
    ThemeResources,
};
use waos_package::ProgramId;

use crate::logic::icon_rows::{IconPlace, IconRows, Step, first_row};
use crate::logic::setting_changes::SettingsOf;
use crate::state::launcher::{Launcher, LauncherShown};
use crate::state::library::{Library, LibraryPage};
use crate::state::recordings::Recordings;
use crate::ui::library::home::fluent::{FluentHomeBody, MARGIN, columns_in, show};
use crate::ui::library::home::icon_grid::Highlighted;
use crate::ui::library::home::{HomeEntry, home_entries, often_used_entries};
use crate::ui::library::search_field::{search_token, takes_back_a_token, with_no_box};
use crate::ui::library::words;

/// The size of the search row's text and icons.
const SEARCH_FONT_SIZE: f64 = 18.0;
const ICON_SIZE: f64 = 18.0;

/// The launcher's home, shown while the library is on its grid.
#[derive(Debug)]
pub(crate) struct LauncherHome {
    pub(crate) launcher: Entity<Launcher>,
    pub(crate) library: Entity<Library>,
    pub(crate) recordings: Entity<Recordings>,
}

impl StatefulWidget for LauncherHome {
    type State = LauncherHomeState;

    fn create_state(&self) -> LauncherHomeState {
        LauncherHomeState {
            state: StateData::new(),
            field: None,
            menu: None,
            followed: Vec::new(),
            highlighted: None,
            highlight_key: GlobalKey::new(),
        }
    }
}

pub(crate) struct LauncherHomeState {
    state: StateData<LauncherHome>,
    field: Option<SearchField>,
    /// The menu of "…".
    menu: Option<Ref<MenuFlyout>>,
    /// The library's and the launcher's changes, heard while the home shows.
    followed: Vec<Subscription>,
    /// The icon the arrow keys are on; none while the keys are the search field's.
    highlighted: Option<IconPlace>,
    /// What the highlight is built under, by which it is scrolled into sight.
    highlight_key: GlobalKey,
}

/// What is typed in the search field, and what gives it the keyboard.
struct SearchField {
    text: Ref<TextEditingController>,
    focus: Ref<FocusNode>,
}

impl State for LauncherHomeState {
    type Widget = LauncherHome;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let (launcher, library) = {
            let widget = self.widget(app);
            (widget.launcher.clone(), widget.library.clone())
        };
        let text = TextEditingController::new(app);
        let searched = library.read(app).search().to_owned();
        text.set_text(app, searched);
        text.add_listener(app, Listener::new(move |app| self.typed(app)));
        let field = SearchField {
            text,
            focus: FocusNode::new(app),
        };
        let menu = MenuFlyout::new(app, self.menu_items(app));
        let followed = vec![
            app.observe(&library, move |app| self.search_changed_elsewhere(app)),
            app.subscribe(&launcher, move |app, _: &LauncherShown| {
                self.give_the_keyboard_after_this_frame(app);
                self.read_often_used(app);
                self.clear_highlight(app);
            }),
        ];
        let state = app.get_mut(self);
        state.field = Some(field);
        state.menu = Some(menu);
        state.followed = followed;
        self.give_the_keyboard_after_this_frame(app);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        app.get_mut(self).followed.clear();
        if let Some(menu) = app.get_mut(self).menu.take() {
            menu.dispose(app);
        }
        if let Some(field) = app.get_mut(self).field.take() {
            field.text.dispose(app);
            field.focus.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let widget = self.widget(app);
        let state = app.get(self);
        let highlighted = state.highlighted.map(|place| Highlighted {
            place,
            key: state.highlight_key.clone(),
        });
        let body = FluentHomeBody {
            library: widget.library.clone(),
            recordings: widget.recordings.clone(),
            highlighted,
        };
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                self.search_row(app, &resources),
                Expanded::new(body).into_widget(),
            ])
            .into_widget()
    }
}

impl LauncherHomeState {
    fn field(self: Handle<Self>, app: &App) -> &SearchField {
        app.get(self).field.as_ref().expect("made in init_state")
    }

    /// What is typed in the search field.
    fn text(self: Handle<Self>, app: &App) -> Handle<TextEditingController> {
        self.field(app).text.handle()
    }

    /// What gives the search field the keyboard.
    fn focus(self: Handle<Self>, app: &App) -> Handle<FocusNode> {
        self.field(app).focus.handle()
    }

    /// The magnifier, the category chosen as a token, the field and "…".
    fn search_row(self: Handle<Self>, app: &App, resources: &ThemeResources) -> WidgetRef {
        let field = self.field(app);
        let category = self.widget(app).library.read(app).category();
        let magnifier = FontIcon::symbol(FluentSymbol::Search)
            .font_size(ICON_SIZE)
            .foreground(resources.text_fill_color_secondary);
        // No caret blinks in the empty field: it shows once something is typed, or after a
        // token, which it can take back. The field has the keyboard all along, so the first
        // key, an input method's among them, is its.
        let something_typed = !field.text.text_value(app).is_empty();
        // A token says what is searched already, as macOS's Apps has it: no placeholder after it.
        let placeholder = if category.is_some() {
            ""
        } else {
            words::SEARCH_PROGRAMS
        };
        let text_box = TextBox::new(field.text.erase())
            .focus_node(field.focus.erase())
            .placeholder_text(placeholder)
            .font_size(SEARCH_FONT_SIZE)
            .show_cursor(something_typed || category.is_some());
        let opening_the_first = Focus::new(with_no_box(text_box))
            .skip_traversal(true)
            .can_request_focus(false)
            .on_key_event(Rc::new(move |app, _, event| self.key_pressed(app, event)));
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            MARGIN,
            16.0,
            MARGIN - 8.0,
            16.0,
        ))
        .child(
            Row::new().spacing(12.0).children(
                [magnifier.into_widget()]
                    .into_iter()
                    .chain(category.map(|category| {
                        search_token(words::category(category), SEARCH_FONT_SIZE, resources)
                    }))
                    .chain([
                        Expanded::new(opening_the_first).into_widget(),
                        self.more_button(),
                    ]),
            ),
        )
        .into_widget()
    }

    /// "…", which opens its menu under it.
    fn more_button(self: Handle<Self>) -> WidgetRef {
        FlyoutTarget::new(Builder::new(move |_app, target| {
            let open = Listener::new(move |app| {
                if let Some(menu) = app.get(self).menu.handle() {
                    menu.show_at(app, target);
                }
            });
            Button::new(
                FontIcon::symbol(FluentSymbol::More).font_size(ICON_SIZE),
                open,
            )
            .style(ButtonStyle::Subtle)
            .padding([8.0, 8.0, 8.0, 8.0])
            .into_widget()
        }))
        .into_widget()
    }

    /// The menu of "…": getting programs, and every program's settings.
    fn menu_items(self: Handle<Self>, app: &App) -> Vec<MenuFlyoutItem> {
        let library = self.widget(app).library.clone();
        vec![
            MenuFlyoutItem::new(words::GET_PROGRAMS, show(&library, LibraryPage::AppStore)),
            MenuFlyoutItem::new(
                words::SETTINGS,
                show(&library, LibraryPage::Settings(SettingsOf::Global)),
            ),
        ]
    }

    /// A key pressed in the field: Backspace in an empty field takes back the category chosen,
    /// as it takes back a character; an arrow moves the highlight over the icons, as Spotlight's
    /// and GNOME's app grid's do, Down entering them and Up from the top row leaving them for
    /// the field; Enter opens the icon highlighted, or the first shown; every other key goes on,
    /// Left and Right among them while no icon is highlighted, where they are the field's.
    fn key_pressed(self: Handle<Self>, app: &mut App, event: &KeyEvent) -> KeyEventResult {
        if !matches!(event, KeyEvent::Down(_) | KeyEvent::Repeat(_)) {
            return KeyEventResult::Ignored;
        }
        let typed = self.text(app).text_value(app).to_owned();
        if takes_back_a_token(event, &typed) && self.takes_back_the_category(app) {
            return KeyEventResult::Handled;
        }
        let key = event.logical_key();
        if [LogicalKeyboardKey::ENTER, LogicalKeyboardKey::NUMPAD_ENTER].contains(&key) {
            self.open_highlighted_or_first(app);
            return KeyEventResult::Handled;
        }
        let step = match key {
            LogicalKeyboardKey::ARROW_UP => Step::Up,
            LogicalKeyboardKey::ARROW_DOWN => Step::Down,
            LogicalKeyboardKey::ARROW_LEFT => Step::Left,
            LogicalKeyboardKey::ARROW_RIGHT => Step::Right,
            _ => return KeyEventResult::Ignored,
        };
        let highlighted = app.get(self).highlighted;
        if highlighted.is_none() && step != Step::Down {
            return KeyEventResult::Ignored;
        }
        let moved = self.icon_rows(app).after(highlighted, step);
        self.set_state(app, |state| state.highlighted = moved);
        self.bring_highlight_into_sight_after_this_frame(app, step);
        KeyEventResult::Handled
    }

    /// Shows every category's apps again where one category's show alone; says whether one's
    /// did.
    fn takes_back_the_category(self: Handle<Self>, app: &mut App) -> bool {
        let library = self.widget(app).library.clone();
        if library.read(app).category().is_none() {
            return false;
        }
        library.update(app, |library, cx| library.show_category(None, cx));
        self.clear_highlight(app);
        true
    }

    /// The home's rows of icons as it shows them now, each icon by its app's id.
    fn icon_rows(self: Handle<Self>, app: &mut App) -> IconRows<ProgramId> {
        let width = MediaQuery::size_of(app, self.context(app)).width();
        let columns = columns_in(width);
        let library = self.widget(app).library.read(app);
        let ids = |entries: Vec<HomeEntry>| -> Vec<ProgramId> {
            entries.into_iter().map(|entry| entry.id).collect()
        };
        let apps = ids(home_entries(library));
        let often_used = ids(often_used_entries(library));
        IconRows::new(first_row(&often_used, apps.len(), columns), &apps, columns)
    }

    /// Opens the app whose icon is highlighted, or the first shown where none is.
    fn open_highlighted_or_first(self: Handle<Self>, app: &mut App) {
        let rows = self.icon_rows(app);
        let place = app
            .get(self)
            .highlighted
            .unwrap_or(IconPlace { row: 0, column: 0 });
        let Some(id) = rows.at(place).cloned() else {
            return;
        };
        let library = self.widget(app).library.clone();
        library.update(app, |library, cx| library.open(&id, cx));
    }

    /// Scrolls the icon highlighted into sight once this frame has built its highlight: to the
    /// grid's end for a step down the icons, to its start for one up them.
    fn bring_highlight_into_sight_after_this_frame(self: Handle<Self>, app: &mut App, step: Step) {
        let policy = match step {
            Step::Down | Step::Right => ScrollPositionAlignmentPolicy::KeepVisibleAtEnd,
            Step::Up | Step::Left => ScrollPositionAlignmentPolicy::KeepVisibleAtStart,
        };
        let callback = FrameCallback::new(move |app, _| {
            if !self.mounted(app) {
                return;
            }
            let Some(highlight) = app.get(self).highlight_key.current_context(app) else {
                return;
            };
            drop(Scrollable::ensure_visible(
                app,
                highlight,
                0.0,
                Duration::ZERO,
                Curves::linear(),
                policy,
            ));
        });
        SchedulerBinding::add_post_frame_callback(app, callback, None);
    }

    /// No icon is highlighted: the keys are the search field's again.
    fn clear_highlight(self: Handle<Self>, app: &mut App) {
        if app.get(self).highlighted.is_some() {
            self.set_state(app, |state| state.highlighted = None);
        }
    }

    /// The launcher came into sight: the library reads again which apps are used often, so the
    /// row is current as it shows and still while it is looked at.
    fn read_often_used(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        let library = self.widget(app).library.clone();
        library.update(app, |library, cx| library.read_often_used(cx));
    }

    /// The library searches for what the field holds now, and the field is built again for it.
    fn typed(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        let typed = self.text(app).text_value(app).to_owned();
        let library = self.widget(app).library.clone();
        let searched_before = library.read(app).search().to_owned();
        library.update(app, |library, cx| library.search_for(&typed, cx));
        // The field shows its caret only while it holds something; and what is searched for
        // changing changes the icons, so none of them is highlighted any more.
        self.set_state(app, |state| {
            if typed != searched_before {
                state.highlighted = None;
            }
        });
    }

    /// The library changed: where its search is no longer what the field holds, it was changed
    /// elsewhere, and the field takes it.
    fn search_changed_elsewhere(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        let searched = self.widget(app).library.read(app).search().to_owned();
        let text = self.text(app);
        if text.text_value(app) != searched {
            text.set_text(app, searched);
        }
    }

    /// The field takes the keyboard once this frame is drawn, the page around it built.
    fn give_the_keyboard_after_this_frame(self: Handle<Self>, app: &mut App) {
        let callback = FrameCallback::new(move |app, _| {
            if self.mounted(app) {
                self.focus(app).request_focus(app, None);
            }
        });
        SchedulerBinding::add_post_frame_callback(app, callback, None);
    }
}
