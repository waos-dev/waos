//! What the window shows: the note or the file the user picks, the first listed when the search or
//! the folder leaves the one shown out, the note a reminder names, wherever it is, and a file as it
//! is opened; and the search, as it is typed.
//!
//! The invariant a later editor will break: what is shown changes only here, and the editor is
//! given its text after the change, or after another app's change to the file shown is read, so
//! the editor's text is always the shown's and an edit is never kept as another's.

use std::path::Path;

use waft_embedder::{TextAffinity, TextEditingValue, TextSelection};
use waft_foundation::{App, Listenable, Listener};

use crate::browsing::{Folder, Shown};
use crate::list_pane::uncover;
use crate::note::NoteId;
use crate::notes_app::NotesApp;
use crate::window::bring_forward;

/// Shows `shown`, or nothing, with its text in the editor from the start, or as it is where it is
/// shown already; a file is looked at again first, so the latest another app wrote is shown;
/// whatever the window said goes.
pub(crate) fn show(app: &mut App, notes: &NotesApp, shown: Option<Shown>) {
    let another = notes.browsing.read(app).shown() != shown.as_ref();
    if another && let Some(Shown::File(path)) = &shown {
        notes.files.update(app, |files, cx| {
            if files.look_again_at(path) {
                cx.notify();
            }
        });
    }
    notes.browsing.update(app, |browsing, cx| {
        browsing.set_shown(shown);
        browsing.clear_notice();
        cx.notify();
    });
    if another {
        give_the_editor(app, notes);
    }
}

/// Shows `shown` where the user sees it: as [`show`], with the list's pane closed where it covers
/// it.
pub(crate) fn show_in_front(app: &mut App, notes: &NotesApp, shown: Shown) {
    show(app, notes, Some(shown));
    uncover(app, notes);
}

/// The file at `path` was read again, as another app changed it: the editor is given its text
/// where it is shown.
pub(crate) fn reread(app: &mut App, notes: &NotesApp, path: &Path) {
    if notes.browsing.read(app).shown_file() == Some(path) {
        give_the_editor(app, notes);
    }
}

/// Gives the editor the text of what is shown, or none, the cursor at its start.
fn give_the_editor(app: &mut App, notes: &NotesApp) {
    let text = match notes.browsing.read(app).shown() {
        Some(Shown::Note(id)) => notes
            .library
            .read(app)
            .note(*id)
            .map(|note| note.text.clone()),
        Some(Shown::File(path)) => notes
            .files
            .read(app)
            .file(path)
            .map(|file| file.text().to_owned()),
        None => None,
    };
    let at_start = TextSelection::collapsed(0, TextAffinity::Downstream);
    let value = TextEditingValue::new()
        .text(text.unwrap_or_default())
        .selection(at_start);
    notes.editor.text.set_value(app, value);
}

/// Keeps something listed shown: where what is shown is not listed, shows the first note listed,
/// or else the first file, or nothing.
pub(crate) fn show_a_listed_item(app: &mut App, notes: &NotesApp) {
    let shown = notes.browsing.read(app).shown().cloned();
    if shown.is_some_and(|shown| is_listed(app, notes, &shown)) {
        return;
    }
    let browsing = notes.browsing.read(app);
    let first_note = browsing
        .listed(notes.library.read(app))
        .first()
        .map(|note| Shown::Note(note.id));
    let first = first_note.or_else(|| {
        let files = browsing.listed_files(notes.files.read(app));
        files
            .first()
            .map(|file| Shown::File(file.path().to_owned()))
    });
    show(app, notes, first);
}

/// Lists `folder`, keeping something of it shown.
pub(crate) fn show_folder(app: &mut App, notes: &NotesApp, folder: Folder) {
    notes.browsing.update(app, |browsing, cx| {
        browsing.set_folder(folder);
        cx.notify();
    });
    show_a_listed_item(app, notes);
}

/// Opens the note `id` wherever it is, in the window brought forward.
pub(crate) fn reveal(app: &mut App, notes: &NotesApp, id: NoteId) {
    if notes.library.read(app).note(id).is_none() {
        return;
    }
    let shown = Shown::Note(id);
    list_with(app, notes, &shown);
    show_in_front(app, notes, shown);
    bring_forward(app);
}

/// Lists the folder `shown` is in, the notes for a file, and clears a search that leaves it out.
pub(crate) fn list_with(app: &mut App, notes: &NotesApp, shown: &Shown) {
    let folder = match shown {
        Shown::Note(id) => {
            let Some(note) = notes.library.read(app).note(*id) else {
                return;
            };
            Folder::of(note)
        }
        Shown::File(_) => Folder::Notes,
    };
    notes.browsing.update(app, |browsing, cx| {
        browsing.set_folder(folder);
        cx.notify();
    });
    if !is_listed(app, notes, shown) {
        notes.search.text.clear(app);
    }
}

/// From now on, what is typed in the search field narrows the list, keeping something listed
/// shown.
pub(crate) fn follow_search(app: &mut App, notes: &NotesApp) {
    let following = notes.downgrade();
    let listener = Listener::new(move |app| {
        if let Some(notes) = following.upgrade(app) {
            searched(app, &notes);
        }
    });
    notes.search.text.add_listener(app, listener);
}

/// The search field changed: a change of its text, not of its selection alone, searches anew.
fn searched(app: &mut App, notes: &NotesApp) {
    let typed = notes.search.text.text_value(app).to_owned();
    if typed == notes.browsing.read(app).searched() {
        return;
    }
    notes.browsing.update(app, |browsing, cx| {
        browsing.set_search(&typed);
        cx.notify();
    });
    show_a_listed_item(app, notes);
}

fn is_listed(app: &App, notes: &NotesApp, shown: &Shown) -> bool {
    let (library, files) = (notes.library.read(app), notes.files.read(app));
    notes.browsing.read(app).lists(shown, library, files)
}
