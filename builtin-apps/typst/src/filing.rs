//! The document and its file: File › New, Open…, Save and Save As…, and the question asked
//! before a document with changes its file does not hold is let go.
//!
//! A file is opened in the system's open dialog, a sheet on the window, asked for a file the
//! program may write, or handed over by the system ([`crate::opening`]); WaOS mounts it at its
//! own path. Saving writes the text whole and safely ([`crate::safe_write`]); an untitled
//! document is first asked a place for in the system's save dialog.

use std::fs;
use std::path::{Path, PathBuf};

use waft::ui::{
    FileDialogError, OpenOptions, PickedFile, SaveOptions, TextAffinity, TextEditingValue,
    TextSelection,
};
use waft::{App, FocusNodeLeaf, Task, TextEditingControllerLeaf};
use waft_winui::InfoBarSeverity;

use crate::document::Document;
use crate::documents_app::DocumentsApp;
use crate::last_document::remember;
use crate::notices::{say, say_nothing};
use crate::pages::typeset_now;
use crate::question::{Answer, ask_to_save};
use crate::safe_write::{temporary_folder, write_safely};
use crate::text_file::typst_markup;
use crate::window::main_view;

/// The extension a document is saved with.
const EXTENSION: &str = "typ";

/// File › New: an empty, untitled document, once the one shown is saved or let go.
pub(crate) fn new_document(app: &mut App, documents: &DocumentsApp) {
    let settled = settle_edits(app, documents);
    let documents = documents.clone();
    app.spawn(async move |cx| {
        if settled.await {
            cx.update(|app| show(app, &documents, Document::untitled(String::new())));
        }
    });
}

/// File › Open…: the document picked in the open dialog, once the one shown is saved or let go.
pub(crate) fn open_document(app: &mut App, documents: &DocumentsApp) {
    let settled = settle_edits(app, documents);
    let documents = documents.clone();
    app.spawn(async move |cx| {
        if !settled.await {
            return;
        }
        let Some(picking) = cx.update(|app| pick_to_open(app, &documents)) else {
            return;
        };
        let picked = picking.await;
        cx.update(|app| {
            if let Some(path) = path_picked(app, &documents, picked) {
                open_file(app, &documents, &path);
            }
        });
    });
}

/// File › Save: writes the document to its file, asking for one where it has none.
pub(crate) fn save_document(app: &mut App, documents: &DocumentsApp) {
    let _saved = save(app, documents);
}

/// File › Save As…: writes the document to a file picked in the save dialog, its file from then
/// on.
pub(crate) fn save_document_as(app: &mut App, documents: &DocumentsApp) {
    let _saved = save_as(app, documents);
}

/// Shows the document the file at `path` holds; says why where it cannot be read.
pub(crate) fn open_file(app: &mut App, documents: &DocumentsApp, path: &Path) {
    match fs::read_to_string(path) {
        Ok(text) => show(app, documents, Document::of_file(path.to_owned(), text)),
        Err(error) => {
            let said = format!("“{}” could not be opened: {error}.", name_of(path));
            say(app, documents, InfoBarSeverity::Error, said);
        }
    }
}

/// Shows `document` in place of the one shown: its text in the editor, its pages set at once.
pub(crate) fn show(app: &mut App, documents: &DocumentsApp, document: Document) {
    let text = document.saved_text().to_owned();
    remember(document.path());
    documents.document.update(app, |shown, cx| {
        *shown = document;
        cx.notify();
    });
    documents.pages.update(app, |pages, _cx| pages.forget());
    documents
        .page_images
        .update(app, |images, _cx| images.forget());
    documents.editor.text.set_value(app, from_its_start(text));
    typeset_now(app, documents);
    say_nothing(app, documents);
    documents.editor.focus.request_focus(app, None);
}

/// `text` in the editor with the caret before its first character, so it shows from its start.
fn from_its_start(text: String) -> TextEditingValue {
    let start = TextSelection::collapsed(0, TextAffinity::Downstream);
    TextEditingValue::new().text(text).selection(start)
}

/// Whether the document shown may be let go: at once where it has no unsaved change; otherwise
/// once the user has it saved, or agrees to lose its changes.
pub(crate) fn settle_edits(app: &mut App, documents: &DocumentsApp) -> Task<bool> {
    let document = documents.document.read(app);
    if !document.is_edited() {
        return Task::ready(true);
    }
    let name = document.name();
    let answered = ask_to_save(app, &documents.navigator, &name);
    let documents = documents.clone();
    app.spawn(async move |cx| match answered.await {
        Answer::Save => cx.update(|app| save(app, &documents)).await,
        Answer::Discard => true,
        Answer::Cancel => false,
    })
}

/// Writes the document to its file, asking for one where it has none; answers whether it is
/// saved.
fn save(app: &mut App, documents: &DocumentsApp) -> Task<bool> {
    match documents.document.read(app).path().map(Path::to_owned) {
        Some(path) => Task::ready(write_to(app, documents, path)),
        None => save_as(app, documents),
    }
}

fn save_as(app: &mut App, documents: &DocumentsApp) -> Task<bool> {
    let Some(picking) = pick_to_save(app, documents) else {
        return Task::ready(false);
    };
    let documents = documents.clone();
    app.spawn(async move |cx| {
        let picked = picking.await;
        cx.update(|app| match path_picked(app, &documents, picked) {
            Some(path) => write_to(app, &documents, path),
            None => false,
        })
    })
}

/// Writes the editor's text to `path`, the document's file from then on; says why where it
/// cannot, and answers whether it did.
fn write_to(app: &mut App, documents: &DocumentsApp, path: PathBuf) -> bool {
    let text = documents.editor.text.text_value(app).to_owned();
    if let Err(error) = write_safely(&path, text.as_bytes(), &temporary_folder()) {
        let said = format!("“{}” could not be saved: {error}.", name_of(&path));
        say(app, documents, InfoBarSeverity::Error, said);
        return false;
    }
    remember(Some(&path));
    let moved = documents.document.update(app, |document, cx| {
        let moved = document.path() != Some(path.as_path());
        document.saved_to(path, text);
        cx.notify();
        moved
    });
    if moved {
        typeset_now(app, documents);
    }
    say_nothing(app, documents);
    true
}

type Picking = waft::ui::FileDialogFuture<Option<PickedFile>>;

fn pick_to_open(app: &mut App, documents: &DocumentsApp) -> Option<Picking> {
    let options = OpenOptions {
        types: vec![typst_markup()],
        writable: true,
        ..OpenOptions::default()
    };
    let parent = main_view(app).map(|view| view.id());
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        say(
            app,
            documents,
            InfoBarSeverity::Error,
            "This system has no open dialog.",
        );
        return None;
    };
    Some(dialogs.open_file(parent, options))
}

fn pick_to_save(app: &mut App, documents: &DocumentsApp) -> Option<Picking> {
    let options = SaveOptions {
        types: vec![typst_markup()],
        suggested_name: Some(format!(
            "{}.{EXTENSION}",
            documents.document.read(app).stem()
        )),
        ..SaveOptions::default()
    };
    let parent = main_view(app).map(|view| view.id());
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        say(
            app,
            documents,
            InfoBarSeverity::Error,
            "This system has no save dialog.",
        );
        return None;
    };
    Some(dialogs.save_file(parent, options))
}

/// The path a dialog answered; none where the user cancelled, or, said, where it failed.
pub(crate) fn path_picked(
    app: &mut App,
    documents: &DocumentsApp,
    picked: Result<Option<PickedFile>, FileDialogError>,
) -> Option<PathBuf> {
    let said = match picked {
        Ok(Some(PickedFile::Path(path))) => return Some(path),
        Ok(None) => return None,
        Ok(Some(_)) => "The system gave a file with no path.".to_owned(),
        Err(error) => format!("The file dialog failed: {error}"),
    };
    say(app, documents, InfoBarSeverity::Error, said);
    None
}

/// The file's name, as the window says it.
pub(crate) fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
