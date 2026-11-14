//! A text file opened from outside the library, edited where it is: its path, which is the same
//! inside the program as on the machine since WaOS mounts it there; its text as edited; and the
//! version of it on disk that Notes last read or wrote, which tells a change another app made from
//! Notes' own. Its text is saved as it is typed, safely, a draft in the program's temporary folder
//! renamed onto it, as TextEdit saves itself.
//!
//! WaOS tells a program nothing when another app changes a file, so Notes looks: before each save,
//! and when asked (`look_again`), as the app comes back to the front. A file another app changed
//! is read again while it holds no edit of Notes', and otherwise waits for the user, as one another
//! app moved or deleted does (`FileProblem`).
//!
//! The invariant a later editor will break: the file is written only while its version on disk is
//! the one Notes last read or wrote, or once the user chose to save over whatever is there.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::safe_write::{temporary_folder, write_safely};
use crate::text_file::name_of;

#[derive(Debug)]
pub(crate) struct OpenFile {
    path: PathBuf,
    text: String,
    /// The version on disk Notes last read or wrote; none where it could not tell after a write,
    /// so whatever is there next is taken as its own.
    known: Option<FileVersion>,
    /// Whether the text holds edits not written to the file.
    unsaved: bool,
    problem: Option<FileProblem>,
}

/// What tells two versions of a file apart without reading it: when it was last modified, and its
/// length, as the system keeps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileVersion {
    modified: Option<SystemTime>,
    length: u64,
}

impl FileVersion {
    fn of(path: &Path) -> io::Result<FileVersion> {
        let metadata = fs::metadata(path)?;
        Ok(FileVersion {
            modified: metadata.modified().ok(),
            length: metadata.len(),
        })
    }
}

/// Why an open file's text is not on disk, or not all of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileProblem {
    /// Another app changed the file since Notes last read or wrote it, while Notes held edits;
    /// nothing is written until the user saves over it or reverts to it.
    ChangedElsewhere,
    /// The file is no longer at its path: another app moved or deleted it. Nothing is written
    /// until the user saves it there again.
    Gone,
    /// The file could not be written, for this reason; Notes tries again at the next edit.
    Unwritable(String),
}

impl fmt::Display for FileProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileProblem::ChangedElsewhere => f.write_str("another app changed it since"),
            FileProblem::Gone => f.write_str("another app moved or deleted it"),
            FileProblem::Unwritable(why) => f.write_str(why),
        }
    }
}

/// What looking at a file again found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Looked {
    /// Its text is as it was.
    Same,
    /// Another app changed it, and Notes read its text again.
    Reread,
}

impl OpenFile {
    /// The text file at `path`, read; an error where it cannot be read, or is not UTF-8 text.
    pub(crate) fn read(path: PathBuf) -> io::Result<OpenFile> {
        let known = FileVersion::of(&path)?;
        let text = fs::read_to_string(&path)?;
        Ok(OpenFile {
            path,
            text,
            known: Some(known),
            unsaved: false,
            problem: None,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Its name, as the list and the window's title say it.
    pub(crate) fn name(&self) -> String {
        name_of(&self.path)
    }

    pub(crate) fn problem(&self) -> Option<&FileProblem> {
        self.problem.as_ref()
    }

    /// Whether it holds edits not written to the file, and why, where it does.
    pub(crate) fn unsaved(&self) -> Option<&FileProblem> {
        self.problem.as_ref().filter(|_| self.unsaved)
    }

    /// Gives it `text`, as typed, and saves it, unless another app changed or removed the file,
    /// which waits for the user.
    pub(crate) fn edit(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        text.clone_into(&mut self.text);
        self.unsaved = true;
        self.save();
    }

    /// Writes its text over whatever is at its path now, as the user chose: over another app's
    /// change, or where another app moved or deleted it.
    pub(crate) fn save_anyway(&mut self) {
        self.write();
    }

    /// Reads the file again, letting go of the edits not written to it.
    pub(crate) fn revert(&mut self) -> io::Result<()> {
        self.reread()?;
        self.unsaved = false;
        Ok(())
    }

    /// Tries again to write it, where it could not be written; another app's change or removal
    /// still waits for the user.
    pub(crate) fn save_unwritten(&mut self) {
        if matches!(self.problem, Some(FileProblem::Unwritable(_))) {
            self.save();
        }
    }

    /// Looks whether another app changed or removed the file: reads one changed again while it
    /// holds no edit of Notes', and marks one changed under an edit, or removed, for the user.
    pub(crate) fn look_again(&mut self) -> Looked {
        if self.problem == Some(FileProblem::ChangedElsewhere) {
            return Looked::Same;
        }
        match FileVersion::of(&self.path) {
            Ok(version) if self.known.is_none_or(|known| known == version) => {
                self.forget_gone();
                Looked::Same
            }
            Ok(_) if !self.unsaved => match self.reread() {
                Ok(()) => Looked::Reread,
                Err(error) => {
                    self.problem = Some(FileProblem::Unwritable(error.to_string()));
                    Looked::Same
                }
            },
            Ok(_) => {
                self.problem = Some(FileProblem::ChangedElsewhere);
                Looked::Same
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.problem = Some(FileProblem::Gone);
                Looked::Same
            }
            Err(error) => {
                tracing::warn!(path = %self.path.display(), %error, "an open file cannot be looked at");
                Looked::Same
            }
        }
    }

    /// Writes its text, where the file on disk is still the version Notes last read or wrote.
    fn save(&mut self) {
        if matches!(
            self.problem,
            Some(FileProblem::ChangedElsewhere | FileProblem::Gone)
        ) {
            return;
        }
        match FileVersion::of(&self.path) {
            Ok(version) if self.known.is_none_or(|known| known == version) => self.write(),
            Ok(_) => self.problem = Some(FileProblem::ChangedElsewhere),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.problem = Some(FileProblem::Gone);
            }
            Err(error) => self.problem = Some(FileProblem::Unwritable(error.to_string())),
        }
    }

    /// Writes its text safely, whatever is on disk, and keeps the version it wrote.
    fn write(&mut self) {
        if let Err(error) = write_safely(&self.path, &self.text, &temporary_folder()) {
            tracing::warn!(path = %self.path.display(), %error, "an open file is not written");
            self.problem = Some(FileProblem::Unwritable(error.to_string()));
            return;
        }
        self.known = FileVersion::of(&self.path)
            .inspect_err(|error| {
                tracing::warn!(path = %self.path.display(), %error, "a file written cannot be looked at");
            })
            .ok();
        self.unsaved = false;
        self.problem = None;
    }

    /// Reads its text and version again.
    fn reread(&mut self) -> io::Result<()> {
        let reread = OpenFile::read(self.path.clone())?;
        self.text = reread.text;
        self.known = reread.known;
        self.problem = None;
        Ok(())
    }

    /// The file is where it was again, as Notes left it: it is no longer gone.
    fn forget_gone(&mut self) {
        if self.problem == Some(FileProblem::Gone) {
            self.problem = None;
            if self.unsaved {
                self.save();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file named `name` holding `text` in a folder made for the test, and the file opened. Each
    /// test names its own: the drafts of every test are in the one temporary folder.
    fn opened(name: &str, text: &str) -> (tempfile::TempDir, OpenFile) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join(name);
        fs::write(&path, text).unwrap();
        let file = OpenFile::read(path).unwrap();
        (folder, file)
    }

    fn on_disk(file: &OpenFile) -> String {
        fs::read_to_string(file.path()).unwrap()
    }

    #[test]
    fn an_edit_is_saved_to_the_file_as_it_is_typed() {
        let (_folder, mut file) = opened("todo.txt", "milk");
        assert_eq!((file.name().as_str(), file.text()), ("todo.txt", "milk"));
        file.edit("eggs, milk");
        assert_eq!(on_disk(&file), "eggs, milk");
        file.edit("eggs, milk, bread");
        assert_eq!(on_disk(&file), "eggs, milk, bread");
        assert_eq!(file.problem(), None);
    }

    #[test]
    fn a_change_another_app_made_is_never_written_over_without_the_user() {
        let (_folder, mut file) = opened("shopping.txt", "milk");
        fs::write(file.path(), "their milk").unwrap();
        file.edit("eggs, milk");
        assert_eq!(on_disk(&file), "their milk", "not written over");
        assert_eq!(file.unsaved(), Some(&FileProblem::ChangedElsewhere));
        file.edit("eggs, milk, bread");
        assert_eq!(on_disk(&file), "their milk", "nor at the next edit");

        file.save_anyway();
        assert_eq!(on_disk(&file), "eggs, milk, bread");
        assert_eq!(file.problem(), None);
    }

    #[test]
    fn revert_takes_the_change_another_app_made() {
        let (_folder, mut file) = opened("recipe.txt", "milk");
        fs::write(file.path(), "their milk").unwrap();
        file.edit("eggs, milk");
        file.revert().unwrap();
        assert_eq!(file.text(), "their milk");
        assert_eq!(file.unsaved(), None);
        file.edit("their milk, bread");
        assert_eq!(on_disk(&file), "their milk, bread", "saved as typed again");
    }

    #[test]
    fn looking_again_rereads_a_change_another_app_made_while_notes_holds_no_edit() {
        let (_folder, mut file) = opened("plans.txt", "milk");
        assert_eq!(file.look_again(), Looked::Same);
        fs::write(file.path(), "their milk").unwrap();
        assert_eq!(file.look_again(), Looked::Reread);
        assert_eq!(file.text(), "their milk");
        assert_eq!(file.problem(), None);
    }

    #[test]
    fn a_file_another_app_removed_waits_for_the_user_to_save_it_again() {
        let (_folder, mut file) = opened("letter.txt", "milk");
        fs::remove_file(file.path()).unwrap();
        assert_eq!(file.look_again(), Looked::Same);
        assert_eq!(file.problem(), Some(&FileProblem::Gone));
        assert_eq!(file.unsaved(), None, "nothing typed is lost yet");
        file.edit("eggs, milk");
        assert!(!file.path().exists(), "not written back unasked");
        assert_eq!(file.unsaved(), Some(&FileProblem::Gone));

        file.save_anyway();
        assert_eq!(on_disk(&file), "eggs, milk");
    }

    #[test]
    fn a_file_that_is_not_text_does_not_open() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("picture.png");
        fs::write(&path, [0x89, 0x50, 0x4e, 0x47, 0xff, 0xfe]).unwrap();
        assert!(OpenFile::read(path).is_err());
    }
}
