//! Edit's commands, sent to a program's view: what a program's canvas does with the text editing
//! intents Waft sends the focused widget when the user chooses an item of Edit's, the platform's,
//! in the menu bar, as AppKit sends `copy:` to the first responder. Each is the program's
//! view's `copy`, `cut`, `paste` or `edit` event, whose paste reads whatever the program's read
//! switch says (the clipboard proposal's decisions 4 and 5).

use std::any::TypeId;
use std::collections::HashMap;
use std::rc::Rc;

use waft::{
    ActionRef, App, CallbackAction, CopySelectionTextIntent, DeleteCharacterIntent, Entity, Intent,
    PasteTextIntent, RedoTextIntent, SelectAllTextIntent, UndoTextIntent,
};
use waos::bindings::wasdk::platform::views::EditCommand;

use crate::link::ViewLink;
use crate::state::shown_view::ShownView;

/// The actions that send `view`'s program Edit's commands, by the intent each answers.
pub(crate) fn edit_actions(app: &mut App, view: &Entity<ShownView>) -> HashMap<TypeId, ActionRef> {
    let mut actions = HashMap::new();
    let copy_or_cut = |view: &dyn ViewLink, intent: &CopySelectionTextIntent| {
        if intent.collapse_selection() {
            view.cut();
        } else {
            view.copy();
        }
    };
    add(app, &mut actions, view, copy_or_cut);
    add(app, &mut actions, view, |view, _: &PasteTextIntent| {
        view.paste()
    });
    add(app, &mut actions, view, |view, _: &UndoTextIntent| {
        view.edit(EditCommand::Undo);
    });
    add(app, &mut actions, view, |view, _: &RedoTextIntent| {
        view.edit(EditCommand::Redo);
    });
    add(app, &mut actions, view, |view, _: &SelectAllTextIntent| {
        view.edit(EditCommand::SelectAll);
    });
    add(
        app,
        &mut actions,
        view,
        |view, _: &DeleteCharacterIntent| {
            view.edit(EditCommand::Delete);
        },
    );
    actions
}

/// Adds the action that does `send` to `view`'s program for the intent `T`.
fn add<T: Intent + 'static>(
    app: &mut App,
    actions: &mut HashMap<TypeId, ActionRef>,
    view: &Entity<ShownView>,
    send: impl Fn(&dyn ViewLink, &T) + 'static,
) {
    let view = view.clone();
    let action = CallbackAction::<T>::new(
        app,
        Rc::new(move |app, intent| {
            send(view.read(app).view(), intent);
            None
        }),
    );
    actions.insert(TypeId::of::<T>(), action.into_erased());
}
