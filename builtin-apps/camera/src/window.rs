//! The window the app is in: its user resizes it no smaller than the controls under the picture
//! need, with a little picture over them. A phone's view is in no window, and nothing is limited.

use std::fmt;
use std::rc::Rc;

use waft::{
    App, BuildContext, Handle, Size, State, StateData, StatefulWidget, View, WidgetRef, WindowRef,
};

/// The smallest the user may make the window, in logical points.
const SMALLEST: Size = Size::new(360.0, 420.0);

/// `child`, with the window it is in kept no smaller than [`SMALLEST`].
pub(crate) struct CameraWindow {
    pub(crate) child: WidgetRef,
}

impl fmt::Debug for CameraWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CameraWindow").finish_non_exhaustive()
    }
}

impl StatefulWidget for CameraWindow {
    type State = CameraWindowState;

    fn create_state(&self) -> CameraWindowState {
        CameraWindowState {
            state: StateData::new(),
            limited: None,
        }
    }
}

pub(crate) struct CameraWindowState {
    state: StateData<CameraWindow>,
    /// The window limited last.
    limited: Option<WindowRef>,
}

impl State for CameraWindowState {
    type Widget = CameraWindow;
    waft::state_accessors!();

    /// The view's metrics change at each resize: only another window is limited anew.
    fn did_change_dependencies(self: Handle<Self>, app: &mut App) {
        let context = self.context(app);
        let Some(window) = View::maybe_of(app, context).and_then(|view| view.window()) else {
            return;
        };
        let limited = &app.get(self).limited;
        if limited.as_ref().is_some_and(|was| Rc::ptr_eq(was, &window)) {
            return;
        }
        if let Some(limits) = window.size_limits() {
            limits.set_min_size(Some(SMALLEST));
        }
        app.get_mut(self).limited = Some(window);
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        self.widget(app).child.clone()
    }
}
