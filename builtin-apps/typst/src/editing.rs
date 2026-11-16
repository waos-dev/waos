//! Following the editor: each change of its text marks the document edited or not and sets the
//! pages again once typing pauses, and each move of the caret is told to the status bar.
//!
//! The text's controller is listened to, not the text box, so a change made by a command, by
//! undo or by an input method is followed as typing is.

use std::cell::RefCell;
use std::rc::Rc;

use waft::{App, ListenableObject, Listener, TextEditingControllerLeaf};

use crate::documents_app::DocumentsApp;
use crate::markup::line_and_column;
use crate::pages::typeset_soon;

/// From now on, the document and the status bar follow the editor's text and caret.
pub(crate) fn follow_editor(app: &mut App, documents: &DocumentsApp) {
    let seen = Rc::new(RefCell::new(String::new()));
    let following = documents.downgrade();
    let listener = Listener::new(move |app| {
        if let Some(documents) = following.upgrade(app) {
            follow_text(app, &documents, &seen);
            follow_caret(app, &documents);
        }
    });
    documents.editor.text.add_listener(app, listener);
}

/// Where the text is no longer the one `seen`, tells the document and has the pages set again.
fn follow_text(app: &mut App, documents: &DocumentsApp, seen: &RefCell<String>) {
    let text = documents.editor.text.text_value(app);
    if *seen.borrow() == text {
        return;
    }
    let text = text.to_owned();
    documents.document.update(app, |document, cx| {
        if document.text_changed(&text) {
            cx.notify();
        }
    });
    *seen.borrow_mut() = text;
    typeset_soon(app, documents);
}

fn follow_caret(app: &mut App, documents: &DocumentsApp) {
    let controller = documents.editor.text.handle();
    let caret = controller.selection(app).extent_offset.max(0) as usize;
    let place = line_and_column(controller.text_value(app), caret);
    documents.caret.update(app, |caret, cx| {
        if *caret != place {
            *caret = place;
            cx.notify();
        }
    });
}
