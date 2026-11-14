//! The kinds of file the app's dialogs offer: Typst's markup to open and save, and PDF to export.

use waft::ui::FileType;

/// Typst's markup, `.typ`.
pub(crate) fn typst_markup() -> FileType {
    FileType {
        label: "Typst Document".to_owned(),
        extensions: vec!["typ".to_owned()],
        mime_types: vec!["text/vnd.typst".to_owned()],
    }
}

pub(crate) fn pdf() -> FileType {
    FileType {
        label: "PDF Document".to_owned(),
        extensions: vec!["pdf".to_owned()],
        mime_types: vec!["application/pdf".to_owned()],
    }
}
