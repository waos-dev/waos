//! The app's state, as every command and every widget reaches it: the library, the files open,
//! what the window shows of them, the editor's and the search field's text and focus, and the
//! navigator of the window's app, which its dialogs are shown over.

use std::fmt;
use std::time::SystemTime;

use waft_foundation::{App, Entity, Handle, Ref};
use waft_widgets::{
    AnyTextEditingController, FocusNode, FocusNodeLeaf, GlobalKey, TextEditingController,
    TextEditingControllerRef,
};
use waft_winui::InfoBarSeverity;

use crate::browsing::{Browsing, Notice};
use crate::library::Library;
use crate::open_file_list::OpenFileList;
use crate::open_files::{OpenFiles, Unreadable};
use crate::store::Store;

#[derive(Clone, Debug)]
pub(crate) struct NotesApp {
    pub(crate) library: Entity<Library>,
    pub(crate) files: Entity<OpenFiles>,
    pub(crate) browsing: Entity<Browsing>,
    /// The text of the note or the file shown, which only `showing` sets and `editing` keeps as it
    /// is typed.
    pub(crate) editor: TextAndFocus,
    pub(crate) search: TextAndFocus,
    pub(crate) navigator: GlobalKey,
}

/// The app as a listener on one of its own text fields keeps it: the fields by their bare
/// handles, since a listener registered on a field must not keep that field, and the rest as the
/// app has it.
pub(crate) struct WeakNotesApp {
    library: Entity<Library>,
    files: Entity<OpenFiles>,
    browsing: Entity<Browsing>,
    editor: WeakTextAndFocus,
    search: WeakTextAndFocus,
    navigator: GlobalKey,
}

impl WeakNotesApp {
    /// The app again, while its text fields are kept.
    pub(crate) fn upgrade(&self, app: &App) -> Option<NotesApp> {
        Some(NotesApp {
            library: self.library.clone(),
            files: self.files.clone(),
            browsing: self.browsing.clone(),
            editor: self.editor.upgrade(app)?,
            search: self.search.upgrade(app)?,
            navigator: self.navigator.clone(),
        })
    }
}

/// A text field's text and focus, which outlive the field: the layout builds the field again as
/// it changes, and a command moves the focus to it.
#[derive(Clone)]
pub(crate) struct TextAndFocus {
    pub(crate) text: TextEditingControllerRef,
    pub(crate) focus: Ref<FocusNode>,
}

impl TextAndFocus {
    fn new(app: &mut App) -> TextAndFocus {
        TextAndFocus {
            text: TextEditingController::new(app).into_erased(),
            focus: FocusNode::new(app),
        }
    }

    /// Moves the focus to the field, once it is built where it is not yet.
    pub(crate) fn take_focus(&self, app: &mut App) {
        self.focus.request_focus(app, None);
    }

    fn downgrade(&self) -> WeakTextAndFocus {
        WeakTextAndFocus {
            text: self.text.handle(),
            focus: self.focus.handle(),
        }
    }
}

/// A [`TextAndFocus`] by its bare handles.
#[derive(Clone, Copy)]
struct WeakTextAndFocus {
    text: AnyTextEditingController,
    focus: Handle<FocusNode>,
}

impl WeakTextAndFocus {
    fn upgrade(self, app: &App) -> Option<TextAndFocus> {
        Some(TextAndFocus {
            text: self.text.upgrade(app)?,
            focus: self.focus.upgrade(app)?,
        })
    }
}

impl fmt::Debug for TextAndFocus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextAndFocus")
            .field("focus", &self.focus)
            .finish_non_exhaustive()
    }
}

impl NotesApp {
    /// The app as a listener registered on one of its text fields keeps it.
    pub(crate) fn downgrade(&self) -> WeakNotesApp {
        WeakNotesApp {
            library: self.library.clone(),
            files: self.files.clone(),
            browsing: self.browsing.clone(),
            editor: self.editor.downgrade(),
            search: self.search.downgrade(),
            navigator: self.navigator.clone(),
        }
    }

    /// The app over the notes the program's `/data` keeps and the files open at its last run,
    /// showing none yet, and those files that could not be opened again.
    pub(crate) fn open(app: &mut App) -> (NotesApp, Vec<Unreadable>) {
        let library = Library::open(Store::in_data(), SystemTime::now());
        let (files, unreadable) = OpenFiles::open(OpenFileList::in_data());
        let notes = NotesApp {
            library: app.new_entity(|_cx| library),
            files: app.new_entity(|_cx| files),
            browsing: app.new_entity(|_cx| Browsing::default()),
            editor: TextAndFocus::new(app),
            search: TextAndFocus::new(app),
            navigator: GlobalKey::labeled("notes"),
        };
        (notes, unreadable)
    }

    /// Says `message` above the note, as `severity` says, until the next change.
    pub(crate) fn say(&self, app: &mut App, severity: InfoBarSeverity, message: impl Into<String>) {
        let notice = Notice {
            severity,
            message: message.into(),
        };
        self.browsing.update(app, |browsing, cx| {
            browsing.say(notice);
            cx.notify();
        });
    }
}
