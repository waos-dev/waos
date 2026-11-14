//! The text files open in Notes beside its notes, the most recently opened first, each edited in
//! place (`open_file`), and their paths kept (`open_file_list`) so they are open again at the next
//! launch.
//!
//! The invariant a later editor will break: the list kept names exactly the files open, so every
//! change to which files are open goes through here, which keeps the list at once.

use std::io;
use std::path::{Path, PathBuf};

use crate::open_file::{FileProblem, Looked, OpenFile};
use crate::open_file_list::OpenFileList;

#[derive(Debug)]
pub(crate) struct OpenFiles {
    /// The most recently opened first.
    files: Vec<OpenFile>,
    list: OpenFileList,
}

/// A file kept open that could not be read again, and why.
#[derive(Debug)]
pub(crate) struct Unreadable {
    pub(crate) path: PathBuf,
    pub(crate) error: io::Error,
}

impl OpenFiles {
    /// The files `list` keeps, each read again, and those that could not be, which are let go.
    pub(crate) fn open(list: OpenFileList) -> (OpenFiles, Vec<Unreadable>) {
        let paths = list.load().unwrap_or_else(|error| {
            tracing::warn!(%error, "the files open cannot be read");
            Vec::new()
        });
        let mut files = Vec::new();
        let mut unreadable = Vec::new();
        for path in paths {
            match OpenFile::read(path.clone()) {
                Ok(file) => files.push(file),
                Err(error) => unreadable.push(Unreadable { path, error }),
            }
        }
        let open = OpenFiles { files, list };
        if !unreadable.is_empty() {
            open.keep_list();
        }
        (open, unreadable)
    }

    pub(crate) fn files(&self) -> &[OpenFile] {
        &self.files
    }

    pub(crate) fn file(&self, path: &Path) -> Option<&OpenFile> {
        self.files.iter().find(|file| file.path() == path)
    }

    /// Opens the text file at `path`, first among the files; one open already stays as it is.
    pub(crate) fn add(&mut self, path: PathBuf) -> io::Result<()> {
        if self.file(&path).is_some() {
            return Ok(());
        }
        let file = OpenFile::read(path)?;
        self.files.insert(0, file);
        self.keep_list();
        Ok(())
    }

    /// Lets the file at `path` go, as it is on disk.
    pub(crate) fn close(&mut self, path: &Path) {
        self.files.retain(|file| file.path() != path);
        self.keep_list();
    }

    /// Gives the file at `path` the text typed, saved as it is.
    pub(crate) fn edit(&mut self, path: &Path, text: &str) {
        if let Some(file) = self.file_mut(path) {
            file.edit(text);
        }
    }

    pub(crate) fn save_anyway(&mut self, path: &Path) {
        if let Some(file) = self.file_mut(path) {
            file.save_anyway();
        }
    }

    pub(crate) fn revert(&mut self, path: &Path) -> io::Result<()> {
        match self.file_mut(path) {
            Some(file) => file.revert(),
            None => Ok(()),
        }
    }

    /// Looks whether another app changed or removed each file; answers the paths of those read
    /// again.
    pub(crate) fn look_again(&mut self) -> Vec<PathBuf> {
        self.files
            .iter_mut()
            .filter_map(|file| {
                (file.look_again() == Looked::Reread).then(|| file.path().to_owned())
            })
            .collect()
    }

    /// Looks whether another app changed or removed the file at `path`; answers whether it was
    /// read again.
    pub(crate) fn look_again_at(&mut self, path: &Path) -> bool {
        self.file_mut(path)
            .is_some_and(|file| file.look_again() == Looked::Reread)
    }

    /// The first file holding edits not written to it, with why.
    pub(crate) fn unsaved(&self) -> Option<(&OpenFile, &FileProblem)> {
        self.files
            .iter()
            .find_map(|file| file.unsaved().map(|why| (file, why)))
    }

    /// Tries again to write each file that could not be written.
    pub(crate) fn save_unwritten(&mut self) {
        for file in &mut self.files {
            file.save_unwritten();
        }
    }

    fn file_mut(&mut self, path: &Path) -> Option<&mut OpenFile> {
        self.files.iter_mut().find(|file| file.path() == path)
    }

    /// Keeps the paths of the files open; where it cannot, they are open in this run alone.
    fn keep_list(&self) {
        let paths = self
            .files
            .iter()
            .map(|file| file.path().to_owned())
            .collect();
        if let Err(error) = self.list.save(paths) {
            tracing::warn!(%error, "the files open are not kept for the next launch");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn names(open: &OpenFiles) -> Vec<String> {
        open.files().iter().map(OpenFile::name).collect()
    }

    #[test]
    fn the_files_open_are_open_again_at_the_next_launch_the_latest_first() {
        let folder = tempfile::tempdir().unwrap();
        let list = || OpenFileList::at(folder.path().join("open-files.json"));
        let (todo, recipe) = (
            folder.path().join("todo.txt"),
            folder.path().join("recipe.txt"),
        );
        fs::write(&todo, "milk").unwrap();
        fs::write(&recipe, "flour").unwrap();
        let (mut open, _) = OpenFiles::open(list());
        open.add(todo.clone()).unwrap();
        open.add(recipe.clone()).unwrap();
        open.add(todo.clone()).unwrap();
        assert_eq!(names(&open), ["recipe.txt", "todo.txt"], "each once");

        let (mut reopened, unreadable) = OpenFiles::open(list());
        assert!(unreadable.is_empty());
        assert_eq!(names(&reopened), ["recipe.txt", "todo.txt"]);
        reopened.close(&recipe);
        let (reopened, _) = OpenFiles::open(list());
        assert_eq!(names(&reopened), ["todo.txt"], "closed for good");
    }

    #[test]
    fn a_file_gone_by_the_next_launch_is_let_go() {
        let folder = tempfile::tempdir().unwrap();
        let list = || OpenFileList::at(folder.path().join("open-files.json"));
        let todo = folder.path().join("todo.txt");
        fs::write(&todo, "milk").unwrap();
        let (mut open, _) = OpenFiles::open(list());
        open.add(todo.clone()).unwrap();
        fs::remove_file(&todo).unwrap();

        let (reopened, unreadable) = OpenFiles::open(list());
        assert!(reopened.files().is_empty());
        assert_eq!(unreadable[0].path, todo);
        assert!(OpenFiles::open(list()).1.is_empty(), "and not tried again");
    }
}
