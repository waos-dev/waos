//! Typesetting: Typst's compiler run over the document's text, in the program's typesetting
//! worker ([`worker`]), answering the pages it set and the problems it found, the window's
//! requests and the worker's replies as [`messages`] say them.
//!
//! The compiler remembers what it worked out (`comemo`), so setting the text again after a small
//! change redoes only what the change reaches; what it has not used for a few runs is let go
//! after each, so its memory does not grow with every key.

mod fonts;
pub(crate) mod messages;
mod problem;
mod system_faces;
mod today;
pub(crate) mod worker;
mod world;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use typst_layout::PagedDocument;

pub(crate) use self::fonts::DocumentFonts;
pub(crate) use self::problem::{Place, Problem};
use self::world::DocumentWorld;

/// How many runs the compiler keeps a result it did not use again.
const RUNS_KEPT: usize = 10;

/// What one run of the compiler made of the text.
pub(crate) struct Typeset {
    /// The pages, where the text has no error.
    pub(crate) pages: Option<PagedDocument>,
    /// The errors, then the warnings.
    pub(crate) problems: Vec<Problem>,
    /// The document's folder, where the text names a file in it and the program has not been
    /// given the folder.
    pub(crate) closed_folder: Option<PathBuf>,
    pub(crate) took: Duration,
}

pub(crate) struct Typesetter {
    world: DocumentWorld,
}

impl Typesetter {
    /// A typesetter of an untitled, empty document, set in `fonts`.
    pub(crate) fn new(fonts: DocumentFonts) -> Typesetter {
        Typesetter {
            world: DocumentWorld::new(fonts),
        }
    }

    /// Makes the document the file at `path`, whose folder its files are found in, or untitled.
    pub(crate) fn set_path(&mut self, path: Option<&Path>) {
        self.world.set_path(path);
    }

    /// Sets `text` as pages.
    pub(crate) fn typeset(&mut self, text: &str) -> Typeset {
        let started = Instant::now();
        self.world.set_text(text);
        let compiled = typst::compile::<PagedDocument>(&self.world);
        typst::comemo::evict(RUNS_KEPT);
        let (pages, errors) = match compiled.output {
            Ok(pages) => (Some(pages), Default::default()),
            Err(errors) => (None, errors),
        };
        let problems = errors
            .iter()
            .chain(&compiled.warnings)
            .map(|diagnostic| Problem::of(diagnostic, &self.world))
            .collect();
        Typeset {
            pages,
            problems,
            closed_folder: self.world.take_closed_folder().map(Path::to_path_buf),
            took: started.elapsed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::welcome::WELCOME;

    #[test]
    fn the_starting_document_is_set_without_a_problem() {
        let typeset = Typesetter::new(DocumentFonts::carried()).typeset(WELCOME);
        assert_eq!(typeset.problems, Vec::new());
        assert_eq!(typeset.pages.map(|pages| pages.pages().len()), Some(2));
    }

    #[test]
    fn an_error_is_placed_at_its_line_and_keeps_the_pages_from_being_set() {
        let typeset = Typesetter::new(DocumentFonts::carried())
            .typeset("First line.\n\nThe #unknown is here.\n");
        assert!(typeset.pages.is_none());
        let [problem] = typeset.problems.as_slice() else {
            panic!("one problem, not {:?}", typeset.problems);
        };
        assert!(problem.is_error);
        assert!(problem.message.contains("unknown"), "{}", problem.message);
        assert_eq!(
            problem.place,
            Place::Text {
                line: 3,
                column: 6,
                utf16: 18..25,
            }
        );
    }

    #[test]
    fn setting_again_after_a_change_gives_the_changed_pages() {
        let mut typesetter = Typesetter::new(DocumentFonts::carried());
        assert!(typesetter.typeset("One page.").pages.is_some());
        let pages = typesetter
            .typeset("One page.\n#pagebreak()\nAnother.")
            .pages;
        assert_eq!(pages.map(|pages| pages.pages().len()), Some(2));
    }

    #[test]
    fn an_untitled_document_is_told_to_be_saved_before_it_reaches_a_file() {
        let typeset = Typesetter::new(DocumentFonts::carried()).typeset("#image(\"chart.png\")");
        let said: Vec<&str> = typeset
            .problems
            .iter()
            .map(|problem| problem.message.as_str())
            .collect();
        assert!(
            said.iter()
                .any(|said| said.contains("save the document first")),
            "{said:?}"
        );
    }

    #[test]
    fn a_file_beside_the_document_is_read_from_its_folder() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("chapter.typ"), "From the chapter.").unwrap();
        let mut typesetter = Typesetter::new(DocumentFonts::carried());
        typesetter.set_path(Some(&folder.path().join("book.typ")));

        let typeset = typesetter.typeset("#include \"chapter.typ\"");
        assert_eq!(typeset.problems, Vec::new());
        assert!(typeset.pages.is_some());
        assert_eq!(typeset.closed_folder, None);
    }
}
