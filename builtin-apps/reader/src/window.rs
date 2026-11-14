//! The app's one window: it opens at [`OPENING_SIZE`], wide enough for the list of feeds beside
//! the items; its user resizes it no smaller than [`MIN_SIZE`], a phone's width, where the feeds
//! are a pane over the items; and a notification's click brings it forward.

use std::rc::Rc;

use waft::ui::{Size, ViewRef};
use waft::{
    App, BuildContext, Handle, State, StateData, StatefulWidget, View, WidgetRef, WindowConfig,
    WindowRef,
};

/// The app's name, and its window's title.
pub(crate) const APP_NAME: &str = "Reader";

/// The smallest the user may make the window, in logical points: a small phone's width, and the
/// height of a few items beneath the bar.
const MIN_SIZE: Size = Size::new(320.0, 420.0);

/// The size the window opens at, in logical points.
const OPENING_SIZE: [f64; 2] = [980.0, 680.0];

/// The window the app asks for.
pub(crate) fn config() -> WindowConfig {
    WindowConfig {
        title: APP_NAME.to_owned(),
        size: OPENING_SIZE,
        ..WindowConfig::default()
    }
}

/// Brings the window to the front, where it has arrived.
pub(crate) fn bring_forward(app: &App) {
    let view: Option<ViewRef> = app.platform().views().first().cloned();
    if let Some(window) = view.and_then(|view| view.window()) {
        window.show();
    }
}

/// `child`, in a window kept no smaller than [`MIN_SIZE`].
#[derive(Debug)]
pub(crate) struct ReaderWindow {
    pub(crate) child: WidgetRef,
}

impl StatefulWidget for ReaderWindow {
    type State = ReaderWindowState;

    fn create_state(&self) -> ReaderWindowState {
        ReaderWindowState {
            state: StateData::new(),
            window: None,
        }
    }
}

pub(crate) struct ReaderWindowState {
    state: StateData<ReaderWindow>,
    /// The window around the view the widget is in, where it is in one.
    window: Option<WindowRef>,
}

impl State for ReaderWindowState {
    type Widget = ReaderWindow;
    waft::state_accessors!();

    /// The view's metrics change at each resize: only another window is limited anew.
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
            limits.set_min_size(Some(MIN_SIZE));
        }
        app.get_mut(self).window = window;
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        self.widget(app).child.clone()
    }
}
