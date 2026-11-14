//! The window the app is in on a desktop: its user resizes it no smaller than the page's parts
//! need to stand clear of each other.

use std::rc::Rc;

use waft::{
    App, BuildContext, Handle, KeyRef, Size, State, StateData, StatefulWidget, View, WidgetRef,
    WindowRef,
};

/// The app's name, the window's title.
pub(crate) const APP_NAME: &str = "Synth";

/// The size the window opens at, and the smallest its user may make it.
pub(crate) const FIRST_SIZE: [f64; 2] = [920.0, 560.0];
const SMALLEST: Size = Size::new(360.0, 520.0);

/// `child`, with the window it is in kept no smaller than [`SMALLEST`].
pub(crate) struct SynthWindow {
    child: WidgetRef,
}

impl SynthWindow {
    pub(crate) fn new(child: WidgetRef) -> SynthWindow {
        SynthWindow { child }
    }
}

impl std::fmt::Debug for SynthWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SynthWindow").finish_non_exhaustive()
    }
}

impl StatefulWidget for SynthWindow {
    type State = SynthWindowState;

    fn key(&self) -> Option<&KeyRef> {
        None
    }

    fn create_state(&self) -> SynthWindowState {
        SynthWindowState {
            state: StateData::new(),
            window: None,
        }
    }
}

pub(crate) struct SynthWindowState {
    state: StateData<SynthWindow>,
    /// The window around the view the widget is in, where it is in one.
    window: Option<WindowRef>,
}

impl State for SynthWindowState {
    type Widget = SynthWindow;
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
            limits.set_min_size(Some(SMALLEST));
        }
        app.get_mut(self).window = window;
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        self.widget(app).child.clone()
    }
}
