//! Tab in the editor: it indents, as in any editor of source text, by two spaces at the caret or
//! in place of what is selected, where a text field would hand the focus to the next control.
//!
//! A text field does not hear Tab as a key: Waft, as Flutter, leaves it to the system's text
//! input, which answers with a move of the focus (`NextFocusIntent`). So the editor is given its
//! own action for that intent, which indents instead.

use std::any::TypeId;
use std::collections::HashMap;
use std::rc::Rc;

use waft::ui::{TextAffinity, TextSelection};
use waft::{
    Action, Actions, AnyAction, App, CallbackAction, Handle, IntoWidget, NextFocusIntent,
    TextEditingControllerLeaf, WidgetRef,
};

use crate::markup::controller::MarkupController;

const INDENT: &str = "  ";

/// The action that indents the text of `controller` at its caret.
pub(crate) fn indent_action(app: &mut App, controller: Handle<MarkupController>) -> AnyAction {
    let indenting = Rc::new(move |app: &mut App, _: &NextFocusIntent| {
        indent(app, controller);
        None
    });
    CallbackAction::<NextFocusIntent>::new(app, indenting).as_action()
}

/// `editor`, with Tab doing `indenting` while the focus is in it.
pub(crate) fn with_tab_indenting(indenting: AnyAction, editor: WidgetRef) -> WidgetRef {
    let actions = HashMap::from([(TypeId::of::<NextFocusIntent>(), indenting)]);
    Actions::new(actions, editor).into_widget()
}

/// Puts an indent in place of the selection, the caret after it.
fn indent(app: &mut App, controller: Handle<MarkupController>) {
    let value = controller.value(app);
    let selection = value.selection;
    if !selection.is_valid() {
        return;
    }
    let after = selection.start() + INDENT.len() as i32;
    let indented = value
        .replaced(selection, INDENT)
        .selection(TextSelection::collapsed(after, TextAffinity::Downstream));
    controller.set_value(app, indented);
}
