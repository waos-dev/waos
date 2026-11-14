//! What Typst's compiler sees of the outside: the document's text as its main source, the files
//! beside the document, the fonts, the program's and the system's, the standard library and today's date.
//!
//! The document's text is kept as one `Source`, replaced in place at each change, so the compiler
//! parses again only what changed and reuses what it laid out before. A file beside the document
//! is read from the document's folder at each compile that asks for it, so a picture changed by
//! another app shows at the next change of the text.
//!
//! WaOS gives a program the file its user opens and nothing else of its folder, so a file beside
//! the document cannot be read until the user gives the folder too: the world notes when a read
//! failed in a folder it cannot list, for the app to offer that.
//!
//! The invariant a later editor will break: the main source's id is made from the document's file
//! name, and its folder is the project's root, so a path in the markup is resolved beside the
//! document; an untitled document has no folder, and reaches no file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

use super::fonts::DocumentFonts;
use super::today::today;

/// The file name an untitled document's source goes by.
const UNTITLED_SOURCE: &str = "/Untitled.typ";

pub(crate) struct DocumentWorld {
    library: LazyHash<Library>,
    fonts: DocumentFonts,
    main: Source,
    /// The folder the document is in, where it has a file.
    folder: Option<PathBuf>,
    /// Whether a file was asked for in the folder while the program could not list it.
    folder_closed: AtomicBool,
}

impl DocumentWorld {
    /// The world of an untitled, empty document, set in `fonts`.
    pub(crate) fn new(fonts: DocumentFonts) -> DocumentWorld {
        DocumentWorld {
            library: LazyHash::new(Library::default()),
            fonts,
            main: Source::new(project_file(UNTITLED_SOURCE), String::new()),
            folder: None,
            folder_closed: AtomicBool::new(false),
        }
    }

    /// Makes the document the file at `path`, or untitled, keeping its text.
    pub(crate) fn set_path(&mut self, path: Option<&Path>) {
        let name = path
            .and_then(Path::file_name)
            .map(|name| format!("/{}", name.to_string_lossy()));
        let id = project_file(name.as_deref().unwrap_or(UNTITLED_SOURCE));
        self.folder = path.and_then(Path::parent).map(Path::to_path_buf);
        if id != self.main.id() {
            self.main = Source::new(id, self.main.text().to_owned());
        }
    }

    /// Makes `text` the document's text.
    pub(crate) fn set_text(&mut self, text: &str) {
        self.main.replace(text);
    }

    pub(crate) fn main_source(&self) -> &Source {
        &self.main
    }

    /// The document's folder, where a file was asked for in it since this was last asked while
    /// the program could not list it.
    pub(crate) fn take_closed_folder(&self) -> Option<&Path> {
        let closed = self.folder_closed.swap(false, Ordering::Relaxed);
        self.folder.as_deref().filter(|_| closed)
    }

    /// The bytes of the file `id` names beside the document.
    fn read(&self, id: FileId) -> FileResult<Vec<u8>> {
        if matches!(id.root(), VirtualRoot::Package(_)) {
            return Err(FileError::Package(PackageError::Other(Some(
                "packages are not available in Documents".into(),
            ))));
        }
        let Some(folder) = &self.folder else {
            return Err(FileError::Other(Some(
                "save the document first: an untitled document has no folder to find files in"
                    .into(),
            )));
        };
        let path = id.vpath().realize(folder).map_err(FileError::Realize)?;
        fs::read(&path).map_err(|error| {
            if fs::read_dir(folder).is_err() {
                self.folder_closed.store(true, Ordering::Relaxed);
            }
            FileError::from_io(error, &path)
        })
    }
}

impl World for DocumentWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            return Ok(self.main.clone());
        }
        let text = String::from_utf8(self.read(id)?).map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, text))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main.id() {
            return Ok(Bytes::from_string(self.main.clone()));
        }
        self.read(id).map(Bytes::new)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        today(offset.map(|offset| offset.seconds() as i64))
    }
}

/// The id of the file at `path`, from the project's root.
fn project_file(path: &str) -> FileId {
    let path = VirtualPath::new(path).unwrap_or_else(|_| {
        VirtualPath::new(UNTITLED_SOURCE).expect("the untitled source's name is a path")
    });
    RootedPath::new(VirtualRoot::Project, path).intern()
}
