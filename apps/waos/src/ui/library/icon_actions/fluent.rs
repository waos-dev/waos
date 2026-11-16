//! A right-clicked icon's actions on a desktop: WinUI's menu flyout at the pointer, with Settings
//! and Remove…, which asks; a click on one closes the menu and does it. The library closes no
//! program on a desktop, where a program's windows are its own to close, so Close is not among
//! them there (`IconAction`).
//!
//! The invariant: the menu's items are read from the library as it opens; the menu is kept for as
//! long as its icon, and let go with it.

use std::rc::Rc;

use waft::{
    App, BuildContext, Builder, Entity, GestureDetector, Handle, IntoWidget, Listener,
    OptionRetained, Ref, State, StateData, StatefulWidget, WidgetRef,
};
use waft_winui::{FlyoutShowOptions, FlyoutTarget, MenuFlyout, MenuFlyoutItem};
use waos_package::ProgramId;

use super::{IconAction, actions_offered};
use crate::state::library::Library;
use crate::ui::library::words;

/// `child`, an icon, with its program's actions at a right click.
#[derive(Debug)]
pub(crate) struct IconMenu {
    pub(crate) library: Entity<Library>,
    pub(crate) id: ProgramId,
    pub(crate) child: WidgetRef,
}

impl StatefulWidget for IconMenu {
    type State = IconMenuState;

    fn create_state(&self) -> IconMenuState {
        IconMenuState {
            state: StateData::new(),
            menu: None,
        }
    }
}

pub(crate) struct IconMenuState {
    state: StateData<IconMenu>,
    menu: Option<Ref<MenuFlyout>>,
}

impl State for IconMenuState {
    type Widget = IconMenu;
    waft_widgets::state_accessors!();

    fn init_state(self: Handle<Self>, app: &mut App) {
        let menu = MenuFlyout::new(app, Vec::new());
        app.get_mut(self).menu = Some(menu);
    }

    fn dispose(self: Handle<Self>, app: &mut App) {
        if let Some(menu) = app.get_mut(self).menu.take() {
            menu.dispose(app);
        }
    }

    fn build(self: Handle<Self>, app: &mut App, _context: BuildContext) -> WidgetRef {
        let child = self.widget(app).child.clone();
        FlyoutTarget::new(Builder::new(move |_app, context| {
            GestureDetector::new()
                .on_secondary_tap_up(Rc::new(move |app, details| {
                    self.open(app, context, details.local_position);
                }))
                .child(child.clone())
                .into_widget()
        }))
        .into_widget()
    }
}

impl IconMenuState {
    /// Opens the menu at `at` in `target`, with what the program is offered now.
    fn open(self: Handle<Self>, app: &mut App, target: BuildContext, at: waft::Offset) {
        let Some(menu) = app.get(self).menu.handle() else {
            return;
        };
        let (library, id) = (
            self.widget(app).library.clone(),
            self.widget(app).id.clone(),
        );
        let items = actions_offered(library.read(app), &id)
            .into_iter()
            .map(|action| {
                let (library, id) = (library.clone(), id.clone());
                let label = match action {
                    IconAction::Remove => words::ASK_TO_REMOVE,
                    action => action.label(),
                };
                MenuFlyoutItem::new(
                    label,
                    Listener::new(move |app| action.carry_out(&library, &id, app)),
                )
            })
            .collect();
        menu.items(app, items);
        let options = FlyoutShowOptions {
            position: Some(at),
            ..FlyoutShowOptions::default()
        };
        menu.show_at_with_options(app, target, options);
    }
}
