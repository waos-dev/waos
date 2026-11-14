//! What the window shows of the library and the files open: the folder listed, Notes, with the
//! files open, or Recently Deleted, narrowed by the search; the note or the file shown, whose text
//! the editor holds; whether the list is a pane over the note, as at a phone's width, and whether
//! that pane is open; and a notice of what a command did, until the next change.
//!
//! The invariant a later editor will break: what is shown changes only through `showing`, which
//! hands the editor its text in the same step.

use std::path::{Path, PathBuf};

use waft_winui::InfoBarSeverity;

use crate::library::Library;
use crate::note::{Note, NoteId};
use crate::open_file::OpenFile;
use crate::open_files::OpenFiles;
use crate::search::Search;

/// A list of notes the sidebar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Folder {
    #[default]
    Notes,
    RecentlyDeleted,
}

impl Folder {
    /// The folder `note` is in.
    pub(crate) fn of(note: &Note) -> Folder {
        if note.is_deleted() {
            Folder::RecentlyDeleted
        } else {
            Folder::Notes
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Folder::Notes => "Notes",
            Folder::RecentlyDeleted => "Recently Deleted",
        }
    }
}

/// What the window shows: a note of the library, or a file open, by its path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Shown {
    Note(NoteId),
    File(PathBuf),
}

#[derive(Debug)]
pub(crate) struct Browsing {
    folder: Folder,
    /// The search as typed.
    searched: String,
    search: Search,
    shown: Option<Shown>,
    /// Whether the list is a pane over the note, which the user opens and closes, rather than a
    /// sidebar beside it.
    narrow: bool,
    /// Whether the list's pane is open: over the note while narrow, beside it otherwise.
    pane_open: bool,
    /// Whether the search field takes the focus once the list's pane has opened.
    search_asked: bool,
    notice: Option<Notice>,
}

/// What a command did, or why it could not, as the window says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Notice {
    pub(crate) severity: InfoBarSeverity,
    pub(crate) message: String,
}

impl Default for Browsing {
    /// As the window's navigation starts, before its first layout: narrow, its pane open.
    fn default() -> Browsing {
        Browsing {
            folder: Folder::default(),
            searched: String::new(),
            search: Search::default(),
            shown: None,
            narrow: true,
            pane_open: true,
            search_asked: false,
            notice: None,
        }
    }
}

impl Browsing {
    pub(crate) fn folder(&self) -> Folder {
        self.folder
    }

    pub(crate) fn searched(&self) -> &str {
        &self.searched
    }

    pub(crate) fn shown(&self) -> Option<&Shown> {
        self.shown.as_ref()
    }

    /// The note shown, where a note is.
    pub(crate) fn shown_note(&self) -> Option<NoteId> {
        match self.shown {
            Some(Shown::Note(id)) => Some(id),
            _ => None,
        }
    }

    /// The path of the file shown, where a file is.
    pub(crate) fn shown_file(&self) -> Option<&Path> {
        match &self.shown {
            Some(Shown::File(path)) => Some(path),
            _ => None,
        }
    }

    pub(crate) fn narrow(&self) -> bool {
        self.narrow
    }

    pub(crate) fn pane_open(&self) -> bool {
        self.pane_open
    }

    pub(crate) fn search_asked(&self) -> bool {
        self.search_asked
    }

    pub(crate) fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// The notes of the folder the search finds, most recently changed first.
    pub(crate) fn listed<'a>(&self, library: &'a Library) -> Vec<&'a Note> {
        library
            .notes()
            .iter()
            .filter(|note| Folder::of(note) == self.folder && self.search.finds(&[&note.text]))
            .collect()
    }

    /// The files open the search finds, among the notes; none in Recently Deleted.
    pub(crate) fn listed_files<'a>(&self, files: &'a OpenFiles) -> Vec<&'a OpenFile> {
        if self.folder != Folder::Notes {
            return Vec::new();
        }
        files
            .files()
            .iter()
            .filter(|file| self.search.finds(&[&file.name(), file.text()]))
            .collect()
    }

    /// Whether `shown` is listed: a note of the folder, or a file, that the search finds.
    pub(crate) fn lists(&self, shown: &Shown, library: &Library, files: &OpenFiles) -> bool {
        match shown {
            Shown::Note(id) => self.listed(library).iter().any(|note| note.id == *id),
            Shown::File(path) => self
                .listed_files(files)
                .iter()
                .any(|file| file.path() == path),
        }
    }

    pub(crate) fn set_folder(&mut self, folder: Folder) {
        self.folder = folder;
    }

    pub(crate) fn set_search(&mut self, typed: &str) {
        typed.clone_into(&mut self.searched);
        self.search = Search::new(typed);
    }

    /// Shows `shown`; only `showing` calls it, which gives the editor its text.
    pub(crate) fn set_shown(&mut self, shown: Option<Shown>) {
        self.shown = shown;
    }

    pub(crate) fn set_narrow(&mut self, narrow: bool) {
        self.narrow = narrow;
    }

    pub(crate) fn set_pane_open(&mut self, open: bool) {
        self.pane_open = open;
    }

    pub(crate) fn set_search_asked(&mut self, asked: bool) {
        self.search_asked = asked;
    }

    /// Says `notice`, until [`clear_notice`](Self::clear_notice).
    pub(crate) fn say(&mut self, notice: Notice) {
        self.notice = Some(notice);
    }

    /// Stops saying the notice; answers whether one was said.
    pub(crate) fn clear_notice(&mut self) -> bool {
        self.notice.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::open_file_list::OpenFileList;
    use crate::store::Store;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn titles(browsing: &Browsing, library: &Library) -> Vec<String> {
        let listed = browsing.listed(library);
        listed.iter().map(|note| note.title().to_owned()).collect()
    }

    #[test]
    fn the_list_is_the_folders_notes_the_search_finds() {
        let folder = tempfile::tempdir().unwrap();
        let mut library = Library::open(Store::at(folder.path().to_path_buf()), at(100));
        library.add("Shopping\nmilk, eggs".to_owned(), at(100));
        library.add("Recipes\neggs, flour".to_owned(), at(200));
        let plans = library.add("Plans".to_owned(), at(300));
        library.delete(plans, at(400));
        let mut browsing = Browsing::default();
        assert_eq!(titles(&browsing, &library), ["Recipes", "Shopping"]);

        browsing.set_search("milk");
        assert_eq!(titles(&browsing, &library), ["Shopping"]);
        browsing.set_search("");
        browsing.set_folder(Folder::RecentlyDeleted);
        assert_eq!(titles(&browsing, &library), ["Plans"]);
        browsing.set_search("eggs");
        assert!(titles(&browsing, &library).is_empty());
    }

    #[test]
    fn the_files_open_are_listed_among_the_notes_as_the_search_finds_them() {
        let folder = tempfile::tempdir().unwrap();
        let (todo, recipe) = (
            folder.path().join("todo.txt"),
            folder.path().join("recipe.txt"),
        );
        std::fs::write(&todo, "milk, eggs").unwrap();
        std::fs::write(&recipe, "flour").unwrap();
        let list = OpenFileList::at(folder.path().join("open-files.json"));
        let (mut files, _) = OpenFiles::open(list);
        files.add(todo.clone()).unwrap();
        files.add(recipe).unwrap();
        let names = |browsing: &Browsing| -> Vec<String> {
            let listed = browsing.listed_files(&files);
            listed.iter().map(|file| file.name()).collect()
        };
        let mut browsing = Browsing::default();
        assert_eq!(names(&browsing), ["recipe.txt", "todo.txt"]);

        browsing.set_search("todo eggs");
        assert_eq!(names(&browsing), ["todo.txt"], "by its name and its text");
        assert!(browsing.lists(
            &Shown::File(todo),
            &Library::open(Store::at(folder.path().join("notes")), at(0)),
            &files
        ));
        browsing.set_search("");
        browsing.set_folder(Folder::RecentlyDeleted);
        assert!(names(&browsing).is_empty(), "not among the deleted");
    }
}
