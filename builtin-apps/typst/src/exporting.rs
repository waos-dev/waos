//! File › Export PDF…: the pages written as a PDF by Typst's own exporter, where the user picks
//! in the system's save dialog, a sheet on the window; WaOS mounts the file writable. The PDF is
//! made of the text as it is, set anew, so it is never of an earlier text, and before the dialog
//! asks, so a text with an error, which is not exported, is said at once. The typesetting worker
//! sets the text and writes the PDF, and the dialog asks once it has.

use std::path::Path;

use waft::App;
use waft::ui::SaveOptions;
use waft_winui::InfoBarSeverity;

use crate::documents_app::DocumentsApp;
use crate::filing::{name_of, path_picked};
use crate::notices::say;
use crate::pages::typeset_now;
use crate::safe_write::{temporary_folder, write_safely};
use crate::text_file;
use crate::typesetting::messages::{PdfRefusal, Request};
use crate::window::main_view;

/// Asks for the document's PDF, of its text as it is: the dialog asks where to keep it once it
/// is made ([`pdf_made`]).
pub(crate) fn export_pdf(app: &mut App, documents: &DocumentsApp) {
    typeset_now(app, documents);
    documents.typesetting.ask(Request::ExportPdf);
}

/// The worker made the PDF, or says why not: asks where to keep it, and writes it there.
pub(crate) fn pdf_made(app: &mut App, documents: &DocumentsApp, pdf: Result<Vec<u8>, PdfRefusal>) {
    let pdf = match pdf {
        Ok(pdf) => pdf,
        Err(refusal) => return say(app, documents, InfoBarSeverity::Warning, said_of(&refusal)),
    };
    let options = SaveOptions {
        types: vec![text_file::pdf()],
        suggested_name: Some(format!("{}.pdf", documents.document.read(app).stem())),
        ..SaveOptions::default()
    };
    let parent = main_view(app).map(|view| view.id());
    let platform = app.platform();
    let Some(dialogs) = platform.file_dialogs() else {
        return say(
            app,
            documents,
            InfoBarSeverity::Error,
            "This system has no save dialog.",
        );
    };
    let picking = dialogs.save_file(parent, options);
    let documents = documents.clone();
    app.spawn(async move |cx| {
        let picked = picking.await;
        cx.update(|app| {
            if let Some(path) = path_picked(app, &documents, picked) {
                keep_at(app, &documents, &pdf, &path);
            }
        });
    });
}

/// Why there is no PDF, in words the window says.
fn said_of(refusal: &PdfRefusal) -> String {
    match refusal {
        PdfRefusal::HasErrors => "The document has errors. Fix them to export a PDF.".to_owned(),
        PdfRefusal::Failed(why) => format!("The PDF could not be made: {why}."),
    }
}

/// Writes `pdf` at `path`, and says how that went.
fn keep_at(app: &mut App, documents: &DocumentsApp, pdf: &[u8], path: &Path) {
    let name = name_of(path);
    let (severity, said) = match write_safely(path, pdf, &temporary_folder()) {
        Ok(()) => (InfoBarSeverity::Success, format!("Exported “{name}”.")),
        Err(error) => (
            InfoBarSeverity::Error,
            format!("“{name}” could not be written: {error}."),
        ),
    };
    say(app, documents, severity, said);
}
