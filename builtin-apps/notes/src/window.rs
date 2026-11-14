//! The app's one window: it opens at [`OPENING_SIZE`], wide enough for the sidebar; its user
//! resizes it no smaller than [`MIN_SIZE`], a phone's width, where the list is a pane over the
//! note; it is titled after the note or the file shown, or `Notes`; and a reminder's click brings
//! it forward.

use std::rc::Rc;

use waft_embedder::{Size, ViewRef, WindowConfig, WindowRef};
use waft_foundation::{App, Handle};
use waft_widgets::{
    BuildContext, IntoWidget, State, StateData, StatefulWidget, StatelessWidget, View, WidgetRef,
};

use crate::browsing::Shown;
use crate::notes_app::NotesApp;

/// The app's name, the window's title while it shows no note.
pub(crate) const APP_NAME: &str = "Notes";

/// The smallest the user may make the window, in logical points: a small phone's width, and the
/// height of the search and a few notes beneath the bar.
const MIN_SIZE: Size = Size::new(320.0, 400.0);

/// The size the window opens at, in logical points.
const OPENING_SIZE: [f64; 2] = [820.0, 540.0];

/// The window the app asks for.
pub(crate) fn config() -> WindowConfig {
    WindowConfig {
        title: APP_NAME.to_owned(),
        size: OPENING_SIZE,
        ..WindowConfig::default()
    }
}

/// The app's one view, its window's, which its dialogs are sheets on, once it has arrived.
pub(crate) fn main_view(app: &App) -> Option<ViewRef> {
    app.platform().views().first().cloned()
}

/// Brings the window to the front, where it has arrived.
pub(crate) fn bring_forward(app: &App) {
    if let Some(window) = main_view(app).and_then(|view| view.window()) {
        window.show();
    }
}

/// `child`, in the window titled after the note or the file shown.
#[derive(Debug)]
pub(crate) struct NotesWindow {
    pub(crate) notes: NotesApp,
    pub(crate) child: WidgetRef,
}

impl StatelessWidget for NotesWindow {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let title = shown_title(app, &self.notes).unwrap_or_else(|| APP_NAME.to_owned());
        TitledWindow {
            title,
            child: self.child.clone(),
        }
        .into_widget()
    }
}

/// The title of what is shown: a note's, or a file's name; none while nothing is shown.
pub(crate) fn shown_title(app: &App, notes: &NotesApp) -> Option<String> {
    match notes.browsing.read(app).shown()? {
        Shown::Note(id) => Some(notes.library.read(app).note(*id)?.title().to_owned()),
        Shown::File(path) => Some(notes.files.read(app).file(path)?.name()),
    }
}

/// `child`, with the window it is in titled `title` and kept no smaller than [`MIN_SIZE`].
#[derive(Debug)]
struct TitledWindow {
    title: String,
    child: WidgetRef,
}

impl StatefulWidget for TitledWindow {
    type State = TitledWindowState;

    fn create_state(&self) -> TitledWindowState {
        TitledWindowState {
            state: StateData::new(),
            window: None,
            titled: None,
        }
    }
}

struct TitledWindowState {
    state: StateData<TitledWindow>,
    /// The window around the view the widget is in, where it is in one.
    window: Option<WindowRef>,
    /// The title the window was given last.
    titled: Option<String>,
}

impl TitledWindowState {
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

impl State for TitledWindowState {
    type Widget = TitledWindow;
    waft_widgets::state_accessors!();

    /// The view's metrics change at each resize: only another window is limited and titled anew.
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
        let state = app.get_mut(self);
        state.window = window;
        state.titled = None;
        self.title_window(app);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, _old_widget: &TitledWindow) {
        self.title_window(app);
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        self.widget(app).child.clone()
    }
}
