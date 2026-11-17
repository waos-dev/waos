//! A held icon's actions on a phone: a `CupertinoActionSheet` from the bottom of the screen,
//! titled with the program's name and its version, with Settings, Close while it runs, and Remove
//! in the destructive colour, and Cancel apart; a tap on one closes the sheet and does it. iOS's
//! home screen shows a context menu over the held icon instead, which Waft has not ported
//! (`CupertinoContextMenu`); the action sheet is the established pattern in its place.
//!
//! The invariant: the sheet reads what it offers from the library each time it builds, so a
//! program that ends while it shows loses Close, and one removed meanwhile closes the sheet.

use waft::{
    App, BuildContext, Entity, IntoWidget, Listener, Navigator, StatelessWidget, Text, WidgetRef,
};
use waft_cupertino::{
    CupertinoActionSheet, CupertinoActionSheetAction, K_CUPERTINO_MODAL_BARRIER_COLOR,
    ShowCupertinoModalPopup,
};
use waos_package::ProgramId;

use super::{IconAction, actions_offered, name_and_version};
use crate::state::library::Library;
use crate::ui::library::words;

/// Shows the actions of the program `id` over everything, from `context`.
pub(crate) fn show_icon_actions(
    app: &mut App,
    context: BuildContext,
    library: &Entity<Library>,
    id: &ProgramId,
) {
    let sheet = IconActionSheet {
        library: library.clone(),
        id: id.clone(),
    };
    // The sheet answers nothing: what it does, it does through the library.
    let _closed = ShowCupertinoModalPopup::new(std::rc::Rc::new(move |_app, _context| {
        sheet.clone().into_widget()
    }))
    .barrier_color(K_CUPERTINO_MODAL_BARRIER_COLOR)
    .semantics_dismissible(true)
    .show(app, context);
}

/// The sheet of a program's actions.
#[derive(Clone, Debug)]
pub(crate) struct IconActionSheet {
    library: Entity<Library>,
    id: ProgramId,
}

impl StatelessWidget for IconActionSheet {
    fn build(&self, app: &mut App, context: BuildContext) -> WidgetRef {
        let library = self.library.read(app);
        let Some((name, version)) = name_and_version(library, &self.id) else {
            return CupertinoActionSheet::new().into_widget();
        };
        let actions = actions_offered(library, &self.id)
            .into_iter()
            .map(|action| self.action(action, context))
            .collect::<Vec<_>>();
        let cancel = Listener::new(move |app| Navigator::pop(app, context, None));
        CupertinoActionSheet::new()
            .title(Text::new(name))
            .message(Text::new(words::version(&version)))
            .actions(actions)
            .cancel_button(CupertinoActionSheetAction::new(
                cancel,
                Text::new(words::CANCEL),
            ))
            .into_widget()
    }
}

impl IconActionSheet {
    /// The button that closes the sheet and does `action`.
    fn action(&self, action: IconAction, context: BuildContext) -> WidgetRef {
        let (library, id) = (self.library.clone(), self.id.clone());
        let pressed = Listener::new(move |app| {
            Navigator::pop(app, context, None);
            action.carry_out(&library, &id, app);
        });
        CupertinoActionSheetAction::new(pressed, Text::new(action.label()))
            .is_destructive_action(action == IconAction::Remove)
            .into_widget()
    }
}
