//! The document: the file it is kept in, where it has one, and whether its text has changes the
//! file does not hold.
//!
//! The invariant a later editor will break: `saved` is the text as last read from or written to
//! the file, or an untitled document's starting text, and `edited` is told from it at each change
//! of the text, so undoing back to it makes the document unedited again.

use std::path::{Path, PathBuf};

/// What a document with no file is called.
const UNTITLED: &str = "Untitled";

#[derive(Debug)]
pub(crate) struct Document {
    path: Option<PathBuf>,
    saved: String,
    edited: bool,
}

impl Document {
    /// A document with no file, starting as `text`.
    pub(crate) fn untitled(text: String) -> Document {
        Document {
            path: None,
            saved: text,
            edited: false,
        }
    }

    /// The document the file at `path` holds as `text`.
    pub(crate) fn of_file(path: PathBuf, text: String) -> Document {
        Document {
            path: Some(path),
            saved: text,
            edited: false,
        }
    }

    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The file's name, or `Untitled`.
    pub(crate) fn name(&self) -> String {
        self.path.as_deref().and_then(Path::file_name).map_or_else(
            || UNTITLED.to_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    /// The file's name without its extension, or `Untitled`: what another file made of the
    /// document is named after.
    pub(crate) fn stem(&self) -> String {
        self.path.as_deref().and_then(Path::file_stem).map_or_else(
            || UNTITLED.to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        )
    }

    /// The text the file holds, or an untitled document started as.
    pub(crate) fn saved_text(&self) -> &str {
        &self.saved
    }

    /// Whether the text has changes the file does not hold.
    pub(crate) fn is_edited(&self) -> bool {
        self.edited
    }

    /// The text is now `text`; answers whether that changed whether the document is edited.
    pub(crate) fn text_changed(&mut self, text: &str) -> bool {
        let edited = text != self.saved;
        let changed = edited != self.edited;
        self.edited = edited;
        changed
    }

    /// The file at `path` now holds `text`, the document's.
    pub(crate) fn saved_to(&mut self, path: PathBuf, text: String) {
        self.path = Some(path);
        self.saved = text;
        self.edited = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_is_edited_while_its_text_differs_from_its_file() {
        let mut document = Document::of_file(PathBuf::from("/letters/anna.typ"), "Dear".into());
        assert!(!document.is_edited());

        assert!(document.text_changed("Dear Anna"));
        assert!(document.is_edited());
        assert!(!document.text_changed("Dear Anna,"));

        assert!(
            document.text_changed("Dear"),
            "undone back to the file's text"
        );
        assert!(!document.is_edited());
    }

    #[test]
    fn saving_names_the_document_after_its_file() {
        let mut document = Document::untitled(String::new());
        assert_eq!(document.name(), "Untitled");
        document.text_changed("= Report");

        document.saved_to(PathBuf::from("/work/report.typ"), "= Report".into());
        assert_eq!(document.name(), "report.typ");
        assert_eq!(document.stem(), "report");
        assert!(!document.is_edited());
    }
}
