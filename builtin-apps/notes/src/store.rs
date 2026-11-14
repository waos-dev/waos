//! Where the notes are kept: in the program's `/data/notes`, one file to a note, named after its id
//! (`1793973930000.json`), holding its text and its times as JSON. A file to a note means a change
//! writes only that note, as it is typed, and a file spoiled or cut short loses only its note,
//! where one file of every note would be written whole at each key and lose them all.
//!
//! The invariant a later editor will break: a note's file is only ever replaced whole, through
//! `safe_write`, with its draft beside it, so a write that fails leaves the note as it was kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::note::{Note, NoteId};
use crate::safe_write::write_safely;

/// The program's own folder the notes are kept in.
const FOLDER: &str = "/data/notes";

/// The extension of a note's file; a draft's, `json.saving`, is not one.
const EXTENSION: &str = "json";

/// The notes' folder.
#[derive(Debug)]
pub(crate) struct Store {
    folder: PathBuf,
}

/// A note's file, its times in milliseconds since the Unix epoch.
#[derive(Debug, Serialize, Deserialize)]
struct NoteFile {
    text: String,
    created: u64,
    changed: u64,
    deleted: Option<u64>,
}

impl Store {
    /// The store in the program's `/data`.
    pub(crate) fn in_data() -> Store {
        Store::at(PathBuf::from(FOLDER))
    }

    /// The store in `folder`.
    pub(crate) fn at(folder: PathBuf) -> Store {
        Store { folder }
    }

    /// Every note kept, making the folder at the first launch; a file that is no note's is left
    /// out, with a warning.
    pub(crate) fn load(&self) -> io::Result<Vec<Note>> {
        fs::create_dir_all(&self.folder)?;
        let mut notes = Vec::new();
        for entry in fs::read_dir(&self.folder)? {
            let path = entry?.path();
            let Some(id) = id_of(&path) else {
                continue;
            };
            match read_note(&path, id) {
                Ok(note) => notes.push(note),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "a note's file is not read")
                }
            }
        }
        Ok(notes)
    }

    /// Keeps `note` as it is now.
    pub(crate) fn save(&self, note: &Note) -> io::Result<()> {
        let file = NoteFile {
            text: note.text.clone(),
            created: millis(note.created),
            changed: millis(note.changed),
            deleted: note.deleted.map(millis),
        };
        let json = serde_json::to_string(&file).map_err(io::Error::other)?;
        write_safely(&self.path_of(note.id), &json, &self.folder)
    }

    /// Lets the note `id` go for good.
    pub(crate) fn remove(&self, id: NoteId) -> io::Result<()> {
        fs::remove_file(self.path_of(id))
    }

    fn path_of(&self, id: NoteId) -> PathBuf {
        self.folder.join(format!("{id}.{EXTENSION}"))
    }
}

/// The id of the note kept at `path`; none for a file that is no note's.
fn id_of(path: &Path) -> Option<NoteId> {
    if path.extension()? != EXTENSION {
        return None;
    }
    path.file_stem()?.to_str()?.parse().ok()
}

fn read_note(path: &Path, id: NoteId) -> io::Result<Note> {
    let file: NoteFile = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(Note {
        id,
        text: file.text,
        created: time(file.created),
        changed: time(file.changed),
        deleted: file.deleted.map(time),
    })
}

fn millis(at: SystemTime) -> u64 {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
}

fn time(millis: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A moment in whole milliseconds, which is what a file keeps.
    fn at(millis: u64) -> SystemTime {
        time(millis)
    }

    fn shopping() -> Note {
        let mut note = Note::new(NoteId(7), "Shopping\nmilk".to_owned(), at(1_000));
        note.changed = at(2_000);
        note
    }

    #[test]
    fn a_note_saved_loads_back_as_it_was() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(folder.path().join("notes"));
        assert!(
            store.load().unwrap().is_empty(),
            "the folder is made, empty"
        );
        let mut deleted = Note::new(NoteId(8), "Plans".to_owned(), at(3_000));
        deleted.deleted = Some(at(4_000));

        store.save(&shopping()).unwrap();
        store.save(&deleted).unwrap();
        let mut loaded = store.load().unwrap();
        loaded.sort_by_key(|note| note.id);
        assert_eq!(loaded, [shopping(), deleted]);
    }

    #[test]
    fn a_note_saved_again_replaces_its_file_and_a_note_removed_is_gone() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(folder.path().to_path_buf());
        let mut note = shopping();
        store.save(&note).unwrap();

        note.text.push_str(", eggs");
        store.save(&note).unwrap();
        assert_eq!(store.load().unwrap(), [note.clone()]);
        let files = fs::read_dir(folder.path()).unwrap().count();
        assert_eq!(files, 1, "one file, and no draft left");
        store.remove(note.id).unwrap();
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn a_file_that_is_no_notes_is_left_out() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(folder.path().to_path_buf());
        store.save(&shopping()).unwrap();
        fs::write(folder.path().join("9.json"), "not json").unwrap();
        fs::write(folder.path().join("7.json.saving"), "a draft").unwrap();
        fs::write(folder.path().join("notes.txt"), "elsewhere").unwrap();

        assert_eq!(store.load().unwrap(), [shopping()]);
    }
}
