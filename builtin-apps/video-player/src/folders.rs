//! The folders the user added to the library, in the order added, kept in the program's own
//! `/data` so that a later run lists them again. Each is a folder the user picked in the system's
//! dialog, which WaOS keeps among the program's files, at the path the pick answered, until the
//! user removes it there.
//!
//! The file holds a folder's path a line. A folder added twice is listed once.

use std::fs;
use std::path::{Path, PathBuf};

/// Where the folders are kept between runs.
const FOLDERS_FILE: &str = "/data/folders";

/// The folders of the library, in the order added.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Folders {
    folders: Vec<PathBuf>,
}

/// The folders as the last run kept them; none at the first.
pub(crate) fn load() -> Folders {
    Folders::read(&fs::read_to_string(FOLDERS_FILE).unwrap_or_default())
}

/// Keeps `folders` for the next run.
pub(crate) fn save(folders: &Folders) {
    if let Err(error) = fs::write(FOLDERS_FILE, folders.text()) {
        eprintln!("the library's folders are not kept: {error}");
    }
}

impl Folders {
    pub(crate) fn list(&self) -> &[PathBuf] {
        &self.folders
    }

    /// Adds `folder` last; false, adding nothing, for one listed already.
    pub(crate) fn add(&mut self, folder: &Path) -> bool {
        if self.folders.iter().any(|listed| listed == folder) {
            return false;
        }
        self.folders.push(folder.to_owned());
        true
    }

    /// Takes away the folder at `index`.
    pub(crate) fn remove(&mut self, index: usize) {
        if index < self.folders.len() {
            self.folders.remove(index);
        }
    }

    /// The folders `text` lists, a path a line.
    fn read(text: &str) -> Folders {
        let mut folders = Folders::default();
        for line in text.lines().filter(|line| !line.is_empty()) {
            folders.add(Path::new(line));
        }
        folders
    }

    /// The folders as their file holds them.
    fn text(&self) -> String {
        self.folders
            .iter()
            .map(|folder| format!("{}\n", folder.display()))
            .collect()
    }
}

/// What the user calls `folder`: its own name, or its whole path for a folder without one.
pub(crate) fn name_of(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folders_read_back_as_they_were_written_each_once() {
        let mut folders = Folders::default();
        assert!(folders.add(Path::new("/Users/a/Movies")));
        assert!(folders.add(Path::new("/Volumes/Films/Holiday 2026")));
        assert!(!folders.add(Path::new("/Users/a/Movies")), "listed already");
        let text = folders.text();
        assert_eq!(text, "/Users/a/Movies\n/Volumes/Films/Holiday 2026\n");
        assert_eq!(Folders::read(&text), folders);
        assert_eq!(Folders::read("/a\n\n/a\n/b\n").list().len(), 2);
    }

    #[test]
    fn a_folder_is_taken_away_by_where_it_is_listed() {
        let mut folders = Folders::read("/a\n/b\n/c\n");
        folders.remove(1);
        folders.remove(7);
        assert_eq!(folders.list(), [PathBuf::from("/a"), PathBuf::from("/c")]);
    }

    #[test]
    fn a_folder_goes_by_its_own_name() {
        assert_eq!(
            name_of(Path::new("/Volumes/Films/Holiday 2026")),
            "Holiday 2026"
        );
        assert_eq!(name_of(Path::new("/")), "/");
    }
}
