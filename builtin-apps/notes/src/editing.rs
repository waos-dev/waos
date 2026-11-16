//! Changing the library and the files open from the window: a new note, opened with the focus in
//! its text; each edit of the note or the file shown, kept as it is typed; deleting the note shown,
//! into Recently Deleted, or from there for good once the user agrees; and recovering one from
//! there.

use std::path::Path;
use std::time::SystemTime;

use waft_foundation::{App, Listenable, Listener};
use waft_winui::InfoBarSeverity;

use crate::browsing::Shown;
use crate::note::NoteId;
use crate::notes_app::NotesApp;
use crate::question::{Question, ask};
use crate::showing::{list_with, show, show_a_listed_item, show_in_front};

/// Adds an empty note among the notes, and opens it with the focus in its text.
pub(crate) fn new_note(app: &mut App, notes: &NotesApp) {
    let id = notes.library.update(app, |library, cx| {
        cx.notify();
        library.add(String::new(), SystemTime::now())
    });
    let shown = Shown::Note(id);
    list_with(app, notes, &shown);
    show_in_front(app, notes, shown);
    notes.editor.take_focus(app);
}

/// Deletes the note shown: into Recently Deleted from the notes, and for good from there, once the
/// user agrees.
pub(crate) fn delete_shown(app: &mut App, notes: &NotesApp) {
    let Some(id) = notes.browsing.read(app).shown_note() else {
        return;
    };
    let Some(note) = notes.library.read(app).note(id) else {
        return;
    };
    if note.is_deleted() {
        ask_to_delete_for_good(app, notes, id);
    } else {
        move_to_recently_deleted(app, notes, id);
    }
}

/// Takes the note shown out of Recently Deleted, back among the notes, and lists it there.
pub(crate) fn recover_shown(app: &mut App, notes: &NotesApp) {
    let Some(id) = deleted_note_shown(app, notes) else {
        return;
    };
    notes.library.update(app, |library, cx| {
        library.recover(id);
        cx.notify();
    });
    list_with(app, notes, &Shown::Note(id));
}

/// From now on, each edit of the note or the file shown is kept as it is typed.
pub(crate) fn keep_edits(app: &mut App, notes: &NotesApp) {
    let following = notes.downgrade();
    let listener = Listener::new(move |app| {
        if let Some(notes) = following.upgrade(app) {
            edited(app, &notes);
        }
    });
    notes.editor.text.add_listener(app, listener);
}

/// The editor changed: a change of its text, not of its selection alone, is the shown's, and
/// whatever the window said goes.
fn edited(app: &mut App, notes: &NotesApp) {
    let changed = match notes.browsing.read(app).shown().cloned() {
        Some(Shown::Note(id)) => edited_note(app, notes, id),
        Some(Shown::File(path)) => edited_file(app, notes, &path),
        None => false,
    };
    if changed {
        notes.browsing.update(app, |browsing, cx| {
            if browsing.clear_notice() {
                cx.notify();
            }
        });
    }
}

/// Keeps the editor's text as the note `id`'s, where it changed; answers whether it did.
fn edited_note(app: &mut App, notes: &NotesApp, id: NoteId) -> bool {
    let text = notes.editor.text.text_value(app);
    if notes
        .library
        .read(app)
        .note(id)
        .is_none_or(|note| note.text == text)
    {
        return false;
    }
    let text = text.to_owned();
    notes.library.update(app, |library, cx| {
        library.set_text(id, &text, SystemTime::now());
        cx.notify();
    });
    true
}

/// Saves the editor's text to the file at `path`, where it changed; answers whether it did.
fn edited_file(app: &mut App, notes: &NotesApp, path: &Path) -> bool {
    let text = notes.editor.text.text_value(app);
    if notes
        .files
        .read(app)
        .file(path)
        .is_none_or(|file| file.text() == text)
    {
        return false;
    }
    let text = text.to_owned();
    notes.files.update(app, |files, cx| {
        files.edit(path, &text);
        cx.notify();
    });
    true
}

fn move_to_recently_deleted(app: &mut App, notes: &NotesApp, id: NoteId) {
    let next = neighbour(app, notes, id);
    notes.library.update(app, |library, cx| {
        library.delete(id, SystemTime::now());
        cx.notify();
    });
    show_next(app, notes, next);
}

/// Asks whether to delete the note for good, and does once the user agrees.
fn ask_to_delete_for_good(app: &mut App, notes: &NotesApp, id: NoteId) {
    let title = notes
        .library
        .read(app)
        .note(id)
        .map(|note| note.title().to_owned())
        .unwrap_or_default();
    let question = Question {
        title: format!("Delete “{title}” for good?"),
        message: "You can’t undo this.".to_owned(),
        confirm: "Delete",
    };
    let agreed = ask(app, &notes.navigator, question);
    let notes = notes.clone();
    app.spawn(async move |cx| {
        if agreed.await {
            cx.update(|app| delete_for_good(app, &notes, id));
        }
    });
}

fn delete_for_good(app: &mut App, notes: &NotesApp, id: NoteId) {
    let next = neighbour(app, notes, id);
    let removed = notes.library.update(app, |library, cx| {
        cx.notify();
        library.remove(id)
    });
    match removed {
        Ok(()) => show_next(app, notes, next),
        Err(error) => {
            let said = format!("The note could not be deleted: {error}");
            notes.say(app, InfoBarSeverity::Error, said);
        }
    }
}

/// The note shown, where it is in Recently Deleted.
fn deleted_note_shown(app: &App, notes: &NotesApp) -> Option<NoteId> {
    let id = notes.browsing.read(app).shown_note()?;
    let note = notes.library.read(app).note(id)?;
    note.is_deleted().then_some(id)
}

/// Shows `next`, the neighbour of a note gone from the list; where it had none, what else is
/// listed.
fn show_next(app: &mut App, notes: &NotesApp, next: Option<NoteId>) {
    match next {
        Some(id) => show(app, notes, Some(Shown::Note(id))),
        None => show_a_listed_item(app, notes),
    }
}

/// The note listed after the note `id`, or before it where it is the last, which is shown once it
/// goes; none where it is the only one.
fn neighbour(app: &App, notes: &NotesApp, id: NoteId) -> Option<NoteId> {
    let listed = notes.browsing.read(app).listed(notes.library.read(app));
    let at = listed.iter().position(|note| note.id == id).unwrap_or(0);
    let others: Vec<NoteId> = listed
        .iter()
        .map(|note| note.id)
        .filter(|&other| other != id)
        .collect();
    others.get(at).or(others.last()).copied()
}
