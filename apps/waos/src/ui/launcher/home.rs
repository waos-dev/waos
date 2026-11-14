//! The launcher's home: a search row as its top, over the home's body (`FluentHomeBody`), the
//! grid of the programs installed with who records above it. The row is a magnifier, the field
//! the user types in, which has the keyboard whenever the launcher shows, and "…", whose menu
//! opens the rest of the library in the same card: getting programs and every program's settings.
//! What is typed is what the library searches its programs for, so the grid filters as the user
//! types, as the phone's home filters by its search pill; Enter opens the first program shown.
//!
//! The invariant: the field holds what the library searches for. What the user types goes to the
//! library, and the library's search changed from elsewhere, as the launcher goes and empties it,
//! is put in the field.

use std::rc::Rc;

use waft::{
    App, BuildContext, Builder, Column, CrossAxisAlignment, EdgeInsetsGeometry, Entity, Expanded,
    Focus, FocusNode, FocusNodeLeaf, FrameCallback, Handle, IntoWidget, KeyEventResult, Listenable,
    Listener, Padding, Row, SchedulerBinding, State, StateData, StatefulWidget, Subscription,
    TextEditingController, TextEditingControllerLeaf, WidgetRef,
};
use waft_services::{KeyEvent, LogicalKeyboardKey};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FlyoutTarget, FontIcon, MenuFlyout, MenuFlyoutItem, TextBox,
    ThemeResources,
};

use crate::logic::setting_changes::SettingsOf;
use crate::state::launcher::{Launcher, WindowStep};
use crate::state::library::{Library, LibraryPage};
use crate::state::recordings::Recordings;
use crate::ui::library::home::fluent::{FluentHomeBody, MARGIN, show};
use crate::ui::library::home::home_entries;
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
            keyboard_given_for: 0,
        }
    }
}

pub(crate) struct LauncherHomeState {
    state: StateData<LauncherHome>,
    field: Option<SearchField>,
    /// The menu of "…".
    menu: Option<Handle<MenuFlyout>>,
    /// The library's and the launcher's changes, heard while the home shows.
    followed: Vec<Subscription>,
    /// The number of the launcher's last showing the field took the keyboard for.
    keyboard_given_for: u64,
}

/// What is typed in the search field, and what gives it the keyboard.
#[derive(Clone, Copy)]
struct SearchField {
    text: Handle<TextEditingController>,
    focus: Handle<FocusNode>,
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
        text.as_controller().set_text(app, searched);
        text.as_controller()
            .add_listener(app, Listener::new(move |app| self.typed(app)));
        let field = SearchField {
            text,
            focus: FocusNode::new(app),
        };
        let menu = MenuFlyout::new(app, self.menu_items(app));
        let followed = vec![
            app.observe(&library, move |app| self.search_changed_elsewhere(app)),
            app.observe(&launcher, move |app| self.launcher_ordered(app)),
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
            app.get_mut(field.text).dispose();
            app.destroy(field.text);
            field.focus.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, context: BuildContext) -> WidgetRef {
        let resources = ThemeResources::of(app, context);
        let widget = self.widget(app);
        let body = FluentHomeBody {
            library: widget.library.clone(),
            recordings: widget.recordings.clone(),
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
    fn field(self: Handle<Self>, app: &App) -> SearchField {
        app.get(self).field.expect("made in init_state")
    }

    /// The magnifier, the field and "…".
    fn search_row(self: Handle<Self>, app: &App, resources: &ThemeResources) -> WidgetRef {
        let field = self.field(app);
        let magnifier = FontIcon::symbol(FluentSymbol::Search)
            .font_size(ICON_SIZE)
            .foreground(resources.text_fill_color_secondary);
        let text_box = TextBox::new(field.text.as_controller())
            .focus_node(field.focus)
            .placeholder_text(words::SEARCH_PROGRAMS)
            .font_size(SEARCH_FONT_SIZE);
        let opening_the_first = Focus::new(text_box)
            .skip_traversal(true)
            .can_request_focus(false)
            .on_key_event(Rc::new(move |app, _, event| self.enter_opens(app, event)));
        Padding::new(EdgeInsetsGeometry::from_ltrb(
            MARGIN,
            16.0,
            MARGIN - 8.0,
            16.0,
        ))
        .child(Row::new().spacing(12.0).children([
            magnifier.into_widget(),
            Expanded::new(opening_the_first).into_widget(),
            self.more_button(),
        ]))
        .into_widget()
    }

    /// "…", which opens its menu under it.
    fn more_button(self: Handle<Self>) -> WidgetRef {
        FlyoutTarget::new(Builder::new(move |_app, target| {
            let open = Listener::new(move |app| {
                if let Some(menu) = app.get(self).menu {
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
            MenuFlyoutItem::new(
                words::GET_PROGRAMS,
                show(&library, LibraryPage::GetPrograms),
            ),
            MenuFlyoutItem::new(
                words::SETTINGS,
                show(&library, LibraryPage::Settings(SettingsOf::Global)),
            ),
        ]
    }

    /// Enter pressed in the field opens the first program shown; every other key goes on.
    fn enter_opens(self: Handle<Self>, app: &mut App, event: &KeyEvent) -> KeyEventResult {
        let enter = [LogicalKeyboardKey::ENTER, LogicalKeyboardKey::NUMPAD_ENTER];
        if !matches!(event, KeyEvent::Down(_)) || !enter.contains(&event.logical_key()) {
            return KeyEventResult::Ignored;
        }
        let library = self.widget(app).library.clone();
        let first = home_entries(library.read(app)).into_iter().next();
        if let Some(first) = first {
            library.update(app, |library, cx| library.open(&first.id, cx));
        }
        KeyEventResult::Handled
    }

    /// The library searches for what the field holds now.
    fn typed(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        let typed = self.field(app).text.text_value(app).to_owned();
        let library = self.widget(app).library.clone();
        library.update(app, |library, cx| library.search_for(&typed, cx));
    }

    /// The library changed: where its search is no longer what the field holds, it was changed
    /// elsewhere, and the field takes it.
    fn search_changed_elsewhere(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        let searched = self.widget(app).library.read(app).search().to_owned();
        let text = self.field(app).text;
        if text.text_value(app) != searched {
            text.as_controller().set_text(app, searched);
        }
    }

    /// The launcher gave its window an order: as it shows, the field takes the keyboard, once
    /// for each showing.
    fn launcher_ordered(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        let Some(order) = self.widget(app).launcher.read(app).order() else {
            return;
        };
        let state = app.get_mut(self);
        if order.step == WindowStep::Show && order.number > state.keyboard_given_for {
            state.keyboard_given_for = order.number;
            self.give_the_keyboard_after_this_frame(app);
        }
    }

    /// The field takes the keyboard once this frame is drawn, the page around it built.
    fn give_the_keyboard_after_this_frame(self: Handle<Self>, app: &mut App) {
        let callback = FrameCallback::new(move |app, _| {
            if self.mounted(app) {
                let focus = self.field(app).focus;
                focus.request_focus(app, None);
            }
        });
        SchedulerBinding::add_post_frame_callback(app, callback, None);
    }
}
