//! File › Export…: the note shown, written as plain text where the user picks in the system's save
//! dialog, a sheet on the window, which WaOS mounts writable, named after its title; or a copy of
//! the file shown, named as it is. The file is written safely, its draft in the program's temporary
//! folder, since the program may not create files beside it.

use std::path::Path;

use waft_embedder::{FileDialogError, PickedFile, SaveOptions};
use waft_foundation::App;
use waft_winui::InfoBarSeverity;

use crate::browsing::Shown;
use crate::notes_app::NotesApp;
use crate::safe_write::{temporary_folder, write_safely};
use crate::text_file::{file_name, name_of, plain_text};
use crate::window::main_view;

/// Asks where to export what is shown, and writes it there.
pub(crate) fn export(app: &mut App, notes: &NotesApp) {
    let Some((text, options)) = exported(app, notes) else {
        return;
    };
    let parent = main_view(app).map(|view| view.id());
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        return notes.say(
            app,
            InfoBarSeverity::Error,
            "This system has no save dialog.",
        );
    };
    let picked = dialogs.save_file(parent, options);
    let notes = notes.clone();
    app.spawn(async move |cx| {
        let picked = picked.await;
        cx.update(|app| export_to_pick(app, &notes, &text, picked));
    });
}

/// The text of what is shown, and how its save dialog asks for a place: by the note's title, or
/// the file's name.
fn exported(app: &App, notes: &NotesApp) -> Option<(String, SaveOptions)> {
    let (text, name) = match notes.browsing.read(app).shown()? {
        Shown::Note(id) => {
            let note = notes.library.read(app).note(*id)?;
            (note.text.clone(), file_name(note.title()))
        }
        Shown::File(path) => {
            let file = notes.files.read(app).file(path)?;
            (file.text().to_owned(), file.name())
        }
    };
    let options = SaveOptions {
        types: vec![plain_text()],
        suggested_name: Some(name),
        ..SaveOptions::default()
    };
    Some((text, options))
}

/// Writes `text` where the user picked, and says how that went.
fn export_to_pick(
    app: &mut App,
    notes: &NotesApp,
    text: &str,
    picked: Result<Option<PickedFile>, FileDialogError>,
) {
    let (severity, said) = match picked {
        Ok(Some(PickedFile::Path(path))) => write_export(&path, text),
        Ok(Some(_)) => (
            InfoBarSeverity::Error,
            "The save dialog gave no path to write.".to_owned(),
        ),
        Ok(None) => return,
        Err(error) => (
            InfoBarSeverity::Error,
            format!("The save dialog failed: {error}"),
        ),
    };
    notes.say(app, severity, said);
}

/// Writes `text` to `path`; answers what the window says of it.
fn write_export(path: &Path, text: &str) -> (InfoBarSeverity, String) {
    let name = name_of(path);
    match write_safely(path, text, &temporary_folder()) {
        Ok(()) => (InfoBarSeverity::Success, format!("Exported to “{name}”.")),
        Err(error) => (
            InfoBarSeverity::Error,
            format!("“{name}” could not be written: {error}"),
        ),
    }
}
