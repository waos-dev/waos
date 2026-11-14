//! Opening text files to edit in place, as a text editor does, beside the notes: File › Open…, the
//! system's open dialog, a sheet on the window, asked for files it may write; and what the system
//! opens Notes with, WaOS's open requests, the one that started the program among them. WaOS mounts
//! each file writable at its own path, and keeps it among the program's mounts. The last file
//! opened is shown, with the focus in its text; a file that is no text is said, and the others open
//! all the same. File › Close File lets the file shown go, asking first where it holds edits not
//! written to it.

use std::path::PathBuf;
use std::rc::Rc;

use waft_embedder::{FileDialogError, OpenOptions, OpenRequest, PickedFile};
use waft_foundation::App;
use waft_winui::InfoBarSeverity;

use crate::browsing::Shown;
use crate::notes_app::NotesApp;
use crate::open_files::Unreadable;
use crate::question::{Question, ask};
use crate::showing::{list_with, show_a_listed_item, show_in_front};
use crate::text_file::{name_of, plain_text};
use crate::window::main_view;

/// Asks for text files to edit, and opens each.
pub(crate) fn open_picked(app: &mut App, notes: &NotesApp) {
    let options = OpenOptions {
        types: vec![plain_text()],
        writable: true,
        ..OpenOptions::default()
    };
    let parent = main_view(app).map(|view| view.id());
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        let said = "This system has no open dialog.";
        return notes.say(app, InfoBarSeverity::Error, said);
    };
    let picked = dialogs.open_files(parent, options);
    let notes = notes.clone();
    app.spawn(async move |cx| {
        let picked = picked.await;
        cx.update(|app| open_dialogs_pick(app, &notes, picked));
    });
}

/// From now on, the files the system opens Notes with are opened, the request that started it
/// among them; a link is none of Notes', and goes unheard.
pub(crate) fn hear_open_requests(app: &mut App, notes: &NotesApp) {
    let notes = notes.clone();
    app.set_open_requested_handler(Some(Rc::new(move |app: &mut App, request: OpenRequest| {
        if !request.links.is_empty() {
            tracing::debug!(links = ?request.links, "Notes opens no links");
        }
        open_files(app, &notes, request.files);
    })));
}

/// Lets the file shown go, once the user agrees where it holds edits not written to it.
pub(crate) fn close_shown_file(app: &mut App, notes: &NotesApp) {
    let Some(path) = notes.browsing.read(app).shown_file().map(PathBuf::from) else {
        return;
    };
    let Some(file) = notes.files.read(app).file(&path) else {
        return;
    };
    let Some(why) = file.unsaved() else {
        return close_file(app, notes, path);
    };
    let question = Question {
        title: format!("Close “{}”?", file.name()),
        message: format!("Its latest changes are not saved, and will be lost: {why}."),
        confirm: "Close File",
    };
    let agreed = ask(app, &notes.navigator, question);
    let notes = notes.clone();
    app.spawn(async move |cx| {
        if agreed.await {
            cx.update(|app| close_file(app, &notes, path));
        }
    });
}

/// Says which of the files open at the last run could not be opened again.
pub(crate) fn say_not_reopened(app: &mut App, notes: &NotesApp, unreadable: &[Unreadable]) {
    if unreadable.is_empty() {
        return;
    }
    let said: Vec<String> = unreadable
        .iter()
        .map(|file| {
            let name = name_of(&file.path);
            format!("“{name}” could not be opened again: {}.", file.error)
        })
        .collect();
    notes.say(app, InfoBarSeverity::Warning, said.join(" "));
}

fn open_dialogs_pick(
    app: &mut App,
    notes: &NotesApp,
    picked: Result<Vec<PickedFile>, FileDialogError>,
) {
    match picked {
        Ok(files) => open_files(app, notes, files),
        Err(error) => {
            let said = format!("The open dialog failed: {error}");
            notes.say(app, InfoBarSeverity::Error, said);
        }
    }
}

/// Opens each of `files`, shows the last opened with the focus in its text, and says what could
/// not be opened.
fn open_files(app: &mut App, notes: &NotesApp, files: Vec<PickedFile>) {
    let mut opened = None;
    let mut problems = Vec::new();
    for file in files {
        match open_file(app, notes, file) {
            Ok(path) => opened = Some(path),
            Err(problem) => problems.push(problem),
        }
    }
    if let Some(path) = opened {
        let shown = Shown::File(path);
        list_with(app, notes, &shown);
        show_in_front(app, notes, shown);
        notes.editor.take_focus(app);
    }
    if !problems.is_empty() {
        notes.say(app, InfoBarSeverity::Error, problems.join(" "));
    }
}

/// Opens `file`, and answers its path, or what the window says of why it cannot be opened.
fn open_file(app: &mut App, notes: &NotesApp, file: PickedFile) -> Result<PathBuf, String> {
    let PickedFile::Path(path) = file else {
        return Err("The system gave a file with no path to open.".to_owned());
    };
    let added = notes.files.update(app, |files, cx| {
        cx.notify();
        files.add(path.clone())
    });
    added
        .map(|()| path.clone())
        .map_err(|error| format!("“{}” could not be opened: {error}.", name_of(&path)))
}

fn close_file(app: &mut App, notes: &NotesApp, path: PathBuf) {
    notes.files.update(app, |files, cx| {
        files.close(&path);
        cx.notify();
    });
    show_a_listed_item(app, notes);
}
