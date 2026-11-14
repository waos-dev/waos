//! What the program opens by itself: at launch, the document open when it last ran, or the
//! starting document; and from then on, the file the system opens Documents with, WaOS's open
//! requests, the one that started the program among them. A file handed over replaces the
//! document shown once that one is saved or let go; of several, the last is opened.

use std::rc::Rc;

use waft::App;
use waft::ui::{OpenRequest, PickedFile};

use crate::document::Document;
use crate::documents_app::DocumentsApp;
use crate::filing::{open_file, settle_edits, show};
use crate::last_document::remembered;
use crate::welcome::WELCOME;
use crate::window::bring_forward;

/// Shows the document of the last run where its file can still be read, the starting document
/// otherwise.
pub(crate) fn open_at_launch(app: &mut App, documents: &DocumentsApp) {
    match remembered().filter(|path| path.is_file()) {
        Some(path) => open_file(app, documents, &path),
        None => show(app, documents, Document::untitled(WELCOME.to_owned())),
    }
}

/// From now on, a file the system opens Documents with is opened; a link is none of its, and
/// goes unheard.
pub(crate) fn hear_open_requests(app: &mut App, documents: &DocumentsApp) {
    let documents = documents.clone();
    app.set_open_requested_handler(Some(Rc::new(move |app: &mut App, request: OpenRequest| {
        open_requested(app, &documents, request);
    })));
}

fn open_requested(app: &mut App, documents: &DocumentsApp, request: OpenRequest) {
    let mut paths = request.files.into_iter().filter_map(|file| match file {
        PickedFile::Path(path) => Some(path),
        _ => None,
    });
    let Some(path) = paths.next_back() else {
        return;
    };
    bring_forward(app);
    let settled = settle_edits(app, documents);
    let documents = documents.clone();
    app.spawn(async move |cx| {
        if settled.await {
            cx.update(|app| open_file(app, &documents, &path));
        }
    });
}
