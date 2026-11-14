//! A reporter of the changes of the window it is drawn in, for state that follows a window: each
//! time the host reports the window changed (minimized or restored, occluded or shown, moved), as
//! the window's scope tells the widgets that read it.
//!
//! The invariant: it reports only while Waft builds frames, as every dependent of the scope
//! rebuilds; what must hear of a window while the app draws nothing reads it again at the app's
//! lifecycle changes.

use std::fmt::{self, Debug};

use waft::{
    App, BuildContext, Handle, Listener, State, StateData, StatefulWidget, WidgetRef, WindowScope,
};

/// Calls `changed` as it is first built in a window, and at each change of the window after.
pub struct WindowChanges {
    changed: Listener,
    child: WidgetRef,
}

impl WindowChanges {
    pub fn new(changed: Listener, child: WidgetRef) -> WindowChanges {
        WindowChanges { changed, child }
    }
}

impl Debug for WindowChanges {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowChanges").finish_non_exhaustive()
    }
}

impl StatefulWidget for WindowChanges {
    type State = WindowChangesState;

    fn create_state(&self) -> WindowChangesState {
        WindowChangesState {
            state: StateData::new(),
        }
    }
}

pub struct WindowChangesState {
    state: StateData<WindowChanges>,
}

impl State for WindowChangesState {
    type Widget = WindowChanges;
    waft_widgets::state_accessors!();

    fn did_change_dependencies(self: Handle<Self>, app: &mut App) {
        if WindowScope::maybe_of(app, self.context(app)).is_some() {
            let changed = self.widget(app).changed.clone();
            changed.call(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        self.widget(app).child.clone()
    }
}
