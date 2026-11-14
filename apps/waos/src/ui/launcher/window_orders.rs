//! What carries out the launcher's orders on its window (`state::launcher`), from inside the
//! window's content, which reaches the window through Waft's `WindowScope`: shown in the middle of
//! the screen the user is on, restored where it was minimized, and given the focus; minimized; or
//! hidden.
//!
//! The invariant: each order is carried out once, by its number, as it is given or, for one given
//! before the window came, as the window first builds.

use std::fmt;

use waft::ui::{ViewFocusDirection, ViewFocusState};
use waft::{
    App, BuildContext, Entity, Handle, State, StateData, StatefulWidget, Subscription, WidgetRef,
    WindowRef, WindowScope,
};

use crate::logic::launcher_placement::launcher_frame;
use crate::logic::launcher_rules::Dismissal;
use crate::state::launcher::{LAUNCHER_SIZE, Launcher, WindowStep};

/// `child`, in the launcher's window, which does what `launcher` orders.
pub(crate) struct WindowOrders {
    pub(crate) launcher: Entity<Launcher>,
    pub(crate) child: WidgetRef,
}

impl fmt::Debug for WindowOrders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowOrders").finish_non_exhaustive()
    }
}

impl StatefulWidget for WindowOrders {
    type State = WindowOrdersState;

    fn create_state(&self) -> WindowOrdersState {
        WindowOrdersState {
            state: StateData::new(),
            window: None,
            carried_out: 0,
            orders: None,
        }
    }
}

pub(crate) struct WindowOrdersState {
    state: StateData<WindowOrders>,
    /// The window the content is drawn in.
    window: Option<WindowRef>,
    /// The number of the last order carried out.
    carried_out: u64,
    orders: Option<Subscription>,
}

impl State for WindowOrdersState {
    type Widget = WindowOrders;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let launcher = self.widget(app).launcher.clone();
        let orders = app.observe(&launcher, move |app| self.carry_out_orders(app));
        app.get_mut(self).orders = Some(orders);
    }

    fn did_change_dependencies(self: Handle<Self>, app: &mut App) {
        let window = WindowScope::maybe_of(app, self.context(app));
        app.get_mut(self).window = window;
        self.carry_out_orders(app);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        app.get_mut(self).orders = None;
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        self.widget(app).child.clone()
    }
}

impl WindowOrdersState {
    /// Carries out the launcher's last order on the window, unless it was carried out already.
    fn carry_out_orders(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        let launcher = self.widget(app).launcher.clone();
        let Some(order) = launcher.read(app).order() else {
            return;
        };
        let state = app.get(self);
        let Some(window) = state.window.clone() else {
            return;
        };
        if order.number <= state.carried_out {
            return;
        }
        app.get_mut(self).carried_out = order.number;
        match order.step {
            WindowStep::Show => show_in_the_middle(app, &window),
            WindowStep::Go(Dismissal::Minimize) => {
                window.set_minimized(true);
                window.show();
            }
            WindowStep::Go(Dismissal::Hide { .. }) => window.hide(),
        }
    }
}

/// Shows `window` in the middle of the screen the user is on, restored where it was minimized,
/// and gives it the focus.
fn show_in_the_middle(app: &App, window: &WindowRef) {
    let platform = app.platform();
    let frame = platform.screens().and_then(|screens| {
        launcher_frame(
            LAUNCHER_SIZE,
            &screens.screens(),
            screens.pointer_location(),
        )
    });
    if let Some(frame) = frame {
        window.set_frame(frame, None);
    }
    window.set_minimized(false);
    window.show();
    window
        .view()
        .host()
        .request_focus_change(ViewFocusState::Focused, ViewFocusDirection::Undefined);
}
