//! The app's state, as every command and every widget reaches it: the document and its file,
//! its pages as Typst set them, their pictures, what the window shows of them, the editor's text
//! and focus, the navigator of the window's app, which its dialogs are shown over, and the
//! typesetting worker, which sets and draws the pages.

use std::fmt;

use waft::{AnyAction, App, Entity, FocusNode, GlobalKey, Handle, ScrollController};

use crate::document::Document;
use crate::indenting::indent_action;
use crate::markup::controller::MarkupController;
use crate::notices::Notice;
use crate::page_images::PageImages;
use crate::pages::Pages;
use crate::typesetting_worker::TypesettingWorker;
use crate::viewing::Viewing;
use crate::welcome::WELCOME;

#[derive(Clone, Debug)]
pub(crate) struct DocumentsApp {
    pub(crate) document: Entity<Document>,
    pub(crate) pages: Entity<Pages>,
    pub(crate) page_images: Entity<PageImages>,
    pub(crate) viewing: Entity<Viewing>,
    /// The caret's line and column in the text, both from 1.
    pub(crate) caret: Entity<(usize, usize)>,
    pub(crate) notice: Entity<Option<Notice>>,
    pub(crate) editor: Editor,
    /// How far down the pages the preview is scrolled, which zooming keeps in place.
    pub(crate) preview_scroll: Handle<ScrollController>,
    pub(crate) navigator: GlobalKey,
    pub(crate) typesetting: TypesettingWorker,
}

/// The editor's text, focus and Tab, which outlive the text box: the layout builds the box
/// again as the window narrows, and a command moves the focus to it.
#[derive(Clone, Copy)]
pub(crate) struct Editor {
    pub(crate) text: Handle<MarkupController>,
    pub(crate) focus: Handle<FocusNode>,
    /// What Tab does in the editor.
    pub(crate) indenting: AnyAction,
}

impl Editor {
    fn new(app: &mut App) -> Editor {
        let text = MarkupController::new(app);
        Editor {
            text,
            focus: FocusNode::new(app),
            indenting: indent_action(app, text),
        }
    }
}

impl fmt::Debug for Editor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Editor")
            .field("text", &self.text)
            .field("focus", &self.focus)
            .finish_non_exhaustive()
    }
}

impl DocumentsApp {
    /// The app showing the starting document, untitled, not yet set as pages, whose pages
    /// `typesetting` sets.
    pub(crate) fn new(app: &mut App, typesetting: TypesettingWorker) -> DocumentsApp {
        DocumentsApp {
            document: app.new_entity(|_cx| Document::untitled(WELCOME.to_owned())),
            pages: app.new_entity(|_cx| Pages::new()),
            page_images: app.new_entity(|_cx| PageImages::default()),
            viewing: app.new_entity(|_cx| Viewing::default()),
            caret: app.new_entity(|_cx| (1, 1)),
            notice: app.new_entity(|_cx| None),
            editor: Editor::new(app),
            preview_scroll: ScrollController::new(app, 0.0, true, None, None, None),
            navigator: GlobalKey::labeled("documents"),
            typesetting,
        }
    }
}
