//! File › Import…: text files the user picks in the system's open dialog, a sheet on the window,
//! each copied into a new note, its text as it is, and the last of them opened; the file is left
//! as it is, where File › Open… edits it in place (`opening`). WaOS mounts each file picked at its
//! path, read-only, as the dialog asked. A file that cannot be read is said, and the others are
//! imported all the same.

use std::fs;
use std::time::SystemTime;

use waft_embedder::{FileDialogError, OpenOptions, PickedFile};
use waft_foundation::App;
use waft_winui::InfoBarSeverity;

use crate::browsing::Shown;
use crate::note::NoteId;
use crate::notes_app::NotesApp;
use crate::showing::{list_with, show_in_front};
use crate::text_file::{name_of, plain_text};
use crate::window::main_view;

/// Asks for text files, and imports each into a note of its own.
pub(crate) fn import(app: &mut App, notes: &NotesApp) {
    let options = OpenOptions {
        types: vec![plain_text()],
        ..OpenOptions::default()
    };
    let parent = main_view(app).map(|view| view.id());
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        return notes.say(
            app,
            InfoBarSeverity::Error,
            "This system has no open dialog.",
        );
    };
    let picked = dialogs.open_files(parent, options);
    let notes = notes.clone();
    app.spawn(async move |cx| {
        let picked = picked.await;
        cx.update(|app| import_picked(app, &notes, picked));
    });
}

/// Imports each file picked, opens the last imported, and says what could not be.
fn import_picked(
    app: &mut App,
    notes: &NotesApp,
    picked: Result<Vec<PickedFile>, FileDialogError>,
) {
    let files = match picked {
        Ok(files) => files,
        Err(error) => {
            let said = format!("The open dialog failed: {error}");
            return notes.say(app, InfoBarSeverity::Error, said);
        }
    };
    let mut imported = None;
    let mut problems = Vec::new();
    for file in files {
        match read(&file) {
            Ok(text) => imported = Some(add_note(app, notes, text)),
            Err(problem) => problems.push(problem),
        }
    }
    if let Some(id) = imported {
        let shown = Shown::Note(id);
        list_with(app, notes, &shown);
        show_in_front(app, notes, shown);
    }
    if !problems.is_empty() {
        notes.say(app, InfoBarSeverity::Error, problems.join(" "));
    }
}

/// The text of `file`, or what the window says of why it cannot be read.
fn read(file: &PickedFile) -> Result<String, String> {
    let PickedFile::Path(path) = file else {
        return Err("The open dialog gave a file with no path to read.".to_owned());
    };
    fs::read_to_string(path)
        .map_err(|error| format!("“{}” could not be imported: {error}.", name_of(path)))
}

fn add_note(app: &mut App, notes: &NotesApp, text: String) -> NoteId {
    notes.library.update(app, |library, cx| {
        cx.notify();
        library.add(text, SystemTime::now())
    })
}
