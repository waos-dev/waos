//! A button that opens a menu of choices anchored to it, as the rate and the tracks are chosen in
//! the controls: WinUI's `DropDownButton` with a `MenuFlyout`, whose items follow the button's.
//!
//! The invariant: the menu is made once for the button, its items replaced as the button's
//! change, and let go with the button.

use std::fmt;

use waft_foundation::{App, Handle, OptionRetained, Ref};
use waft_widgets::{BuildContext, IntoWidget, KeyRef, State, StateData, StatefulWidget, WidgetRef};
use waft_winui::{DropDownButton, MenuFlyout, MenuFlyoutItem};

/// `face`, which opens `items` beneath it.
pub(crate) struct MenuButton {
    face: WidgetRef,
    items: Vec<MenuFlyoutItem>,
}

impl MenuButton {
    pub(crate) fn new(face: WidgetRef, items: Vec<MenuFlyoutItem>) -> MenuButton {
        MenuButton { face, items }
    }
}

impl fmt::Debug for MenuButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MenuButton")
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

impl StatefulWidget for MenuButton {
    type State = MenuButtonState;

    fn key(&self) -> Option<&KeyRef> {
        None
    }

    fn create_state(&self) -> MenuButtonState {
        MenuButtonState {
            state: StateData::new(),
            menu: None,
        }
    }
}

pub(crate) struct MenuButtonState {
    state: StateData<MenuButton>,
    menu: Option<Ref<MenuFlyout>>,
}

impl State for MenuButtonState {
    type Widget = MenuButton;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let items = self.widget(app).items.clone();
        let menu = MenuFlyout::new(app, items);
        app.get_mut(self).menu = Some(menu);
    }

    fn did_update_widget(self: Handle<Self>, app: &mut App, _old_widget: &MenuButton) {
        let items = self.widget(app).items.clone();
        if let Some(menu) = app.get(self).menu.handle() {
            menu.items(app, items);
        }
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(menu) = app.get_mut(self).menu.take() {
            menu.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let face = self.widget(app).face.clone();
        let menu = app.get(self).menu.handle().expect("init_state has run");
        DropDownButton::new(face)
            .flyout(menu.as_flyout(app).clone())
            .into_widget()
    }
}
