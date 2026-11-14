//! The window the app is in, as what plays shapes it: its title says what plays, and its user
//! resizes it no smaller than a small video's room.

use std::rc::Rc;

use waft_embedder::{Size, WindowRef};
use waft_foundation::{App, Handle};
use waft_widgets::{BuildContext, KeyRef, State, StateData, StatefulWidget, View, WidgetRef};

/// The smallest the user may make the window: a 16:9 video's room at 480 by 270.
const SMALLEST: Size = Size::new(480.0, 270.0);

/// The app's name, the window's title while nothing plays.
pub(crate) const APP_NAME: &str = "Video Player";

/// `child`, with the window it is in titled `title` and kept no smaller than [`SMALLEST`].
pub(crate) struct PlayerWindow {
    title: String,
    child: WidgetRef,
}

impl PlayerWindow {
    pub(crate) fn new(title: String, child: WidgetRef) -> PlayerWindow {
        PlayerWindow { title, child }
    }
}

impl std::fmt::Debug for PlayerWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerWindow")
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl StatefulWidget for PlayerWindow {
    type State = PlayerWindowState;

    fn key(&self) -> Option<&KeyRef> {
        None
    }

    fn create_state(&self) -> PlayerWindowState {
        PlayerWindowState {
            state: StateData::new(),
            window: None,
            titled: None,
        }
    }
}

pub(crate) struct PlayerWindowState {
    state: StateData<PlayerWindow>,
    /// The window around the view the widget is in, where it is in one.
    window: Option<WindowRef>,
    /// The title the window was given last.
    titled: Option<String>,
}

impl PlayerWindowState {
    /// Titles the window as the widget says, where the title changed.
    fn title_window(self: Handle<Self>, app: &mut App) {
        let title = self.widget(app).title.clone();
        let state = app.get_mut(self);
        if state.titled.as_ref() == Some(&title) {
            return;
        }
        if let Some(window) = &state.window {
            window.set_title(&title);
            state.titled = Some(title);
        }
    }
}

impl State for PlayerWindowState {
    type Widget = PlayerWindow;
    waft_widgets::state_accessors!();

    /// The view's metrics change at each resize: only another window is limited and titled
    /// anew.
    fn did_change_dependencies(self: Handle<Self>, app: &mut App) {
        let context = self.context(app);
        let window = View::maybe_of(app, context).and_then(|view| view.window());
        let same_window = match (&app.get(self).window, &window) {
            (Some(was), Some(is)) => Rc::ptr_eq(was, is),
            (was, is) => was.is_none() && is.is_none(),
        };
        if same_window {
            return;
        }
        if let Some(limits) = window.as_ref().and_then(|window| window.size_limits()) {
            limits.set_min_size(Some(SMALLEST));
        }
        let state = app.get_mut(self);
        state.window = window;
        state.titled = None;
        self.title_window(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, _old_widget: &PlayerWindow) {
        self.title_window(app);
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        self.widget(app).child.clone()
    }
}

/// The title of a window playing what goes by `name`, or by the `title` its media gives.
pub(crate) fn title_for(name: &str, title: Option<&str>) -> String {
    title
        .filter(|title| !title.is_empty())
        .unwrap_or(name)
        .to_owned()
}
