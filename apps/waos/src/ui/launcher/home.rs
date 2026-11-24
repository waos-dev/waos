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
    App, BuildContext, Builder, Color, ColoredBox, Column, CrossAxisAlignment, EdgeInsetsGeometry,
    Entity, Expanded, Focus, FocusNode, FocusNodeLeaf, FrameCallback, Handle, IntoWidget,
    KeyEventResult, Listenable, Listener, OptionRetained, Padding, Ref, Row, SchedulerBinding,
    SizedBox, State, StateData, StatefulWidget, Subscription, TextEditingController,
    TextEditingControllerLeaf, WidgetRef,
};
use waft_services::{KeyEvent, LogicalKeyboardKey};
use waft_winui::{
    Button, ButtonStyle, FluentSymbol, FlyoutTarget, FontIcon, MenuFlyout, MenuFlyoutItem, TextBox,
    ThemeResources, ThemeScope,
};

use crate::logic::setting_changes::SettingsOf;
use crate::state::launcher::{Launcher, LauncherShown};
use crate::state::library::{Library, LibraryPage};
use crate::state::recordings::Recordings;
use crate::ui::library::home::fluent::{FluentHomeBody, MARGIN, show};
use crate::ui::library::home::home_entries;
use crate::ui::library::words;

/// The size of the search row's text and icons.
const SEARCH_FONT_SIZE: f64 = 18.0;
const ICON_SIZE: f64 = 18.0;
/// The room between the line under the search row and the grid.
const GRID_GAP: f64 = 12.0;

/// `text_box` with no box around it and no accent line under it, focused or not: a launcher's
/// field is the launcher's own surface to type on, as Spotlight's and PowerToys Run's are. Done as
/// WinUI restyles one control, by overriding the resources its states read.
fn with_no_box(text_box: TextBox) -> ThemeScope {
    const NONE: Color = Color::from_argb(0, 0, 0, 0);
    ThemeScope::overriding(text_box).any(|resources| {
        resources.text_control_background = NONE;
        resources.text_control_background_pointer_over = NONE;
        resources.text_control_background_focused = NONE;
        resources.text_control_border_brush = [(0.0, NONE), (1.0, NONE)];
        resources.text_control_border_brush_pointer_over = [(0.0, NONE), (1.0, NONE)];
        resources.text_control_border_brush_focused = [(0.0, NONE), (1.0, NONE)];
        resources.text_control_border_theme_thickness_focused =
            resources.text_control_border_theme_thickness;
    })
}

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
                self.give_the_keyboard_after_this_frame(app)
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
        let body = FluentHomeBody {
            library: widget.library.clone(),
            recordings: widget.recordings.clone(),
        };
        let divider = ColoredBox::new(resources.divider_stroke_color_default)
            .child(SizedBox::new().height(1.0));
        Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                self.search_row(app, &resources),
                divider.into_widget(),
                SizedBox::new().height(GRID_GAP).into_widget(),
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

    /// The magnifier, the field and "…".
    fn search_row(self: Handle<Self>, app: &App, resources: &ThemeResources) -> WidgetRef {
        let field = self.field(app);
        let magnifier = FontIcon::symbol(FluentSymbol::Search)
            .font_size(ICON_SIZE)
            .foreground(resources.text_fill_color_secondary);
        let text_box = TextBox::new(field.text.erase())
            .focus_node(field.focus.erase())
            .placeholder_text(words::SEARCH_PROGRAMS)
            .font_size(SEARCH_FONT_SIZE);
        let opening_the_first = Focus::new(with_no_box(text_box))
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
        let typed = self.text(app).text_value(app).to_owned();
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
