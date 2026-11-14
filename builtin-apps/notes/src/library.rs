//! The library: every note the app keeps, most recently changed first, each change written to the
//! store as it is made, and the notes whose latest change the store could not keep, with why. A
//! note deleted stays, in Recently Deleted, for [`KEPT_DELETED`], and goes for good at the first
//! launch after.
//!
//! The invariant a later editor will break: every change to a note goes through here, which keeps
//! it at once; nothing else writes to the store.

use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::note::{Note, NoteId};
use crate::store::Store;

/// How long a deleted note is kept in Recently Deleted, as Apple's Notes keeps one.
pub(crate) const KEPT_DELETED: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Debug)]
pub(crate) struct Library {
    /// Most recently changed first.
    notes: Vec<Note>,
    store: Store,
    /// Why the store could not keep each note's latest change.
    unsaved: BTreeMap<NoteId, String>,
}

impl Library {
    /// The notes `store` keeps, less those deleted longer ago than [`KEPT_DELETED`], which go for
    /// good; none where the store cannot be read, which is logged.
    pub(crate) fn open(store: Store, now: SystemTime) -> Library {
        let notes = store.load().unwrap_or_else(|error| {
            tracing::warn!(%error, "the notes kept cannot be read");
            Vec::new()
        });
        let mut library = Library {
            notes,
            store,
            unsaved: BTreeMap::new(),
        };
        library.remove_expired(now);
        library.sort();
        library
    }

    pub(crate) fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// How many notes are in Recently Deleted.
    pub(crate) fn deleted_count(&self) -> usize {
        self.notes.iter().filter(|note| note.is_deleted()).count()
    }

    pub(crate) fn note(&self, id: NoteId) -> Option<&Note> {
        self.notes.iter().find(|note| note.id == id)
    }

    /// Adds a note holding `text`, made `now`, and answers its id.
    pub(crate) fn add(&mut self, text: String, now: SystemTime) -> NoteId {
        let id = self.next_id(now);
        self.notes.insert(0, Note::new(id, text, now));
        self.keep(id);
        id
    }

    /// Gives the note `text`, changed `now`, where that is new.
    pub(crate) fn set_text(&mut self, id: NoteId, text: &str, now: SystemTime) {
        let Some(note) = self.note_mut(id) else {
            return;
        };
        if note.text == text {
            return;
        }
        text.clone_into(&mut note.text);
        note.changed = now;
        self.sort();
        self.keep(id);
    }

    /// Puts the note in Recently Deleted, from `now`.
    pub(crate) fn delete(&mut self, id: NoteId, now: SystemTime) {
        if let Some(note) = self.note_mut(id) {
            note.deleted = Some(now);
            self.keep(id);
        }
    }

    /// Takes the note out of Recently Deleted, back among the notes.
    pub(crate) fn recover(&mut self, id: NoteId) {
        if let Some(note) = self.note_mut(id) {
            note.deleted = None;
            self.keep(id);
        }
    }

    /// Lets the note go for good; it stays where the store cannot remove it.
    pub(crate) fn remove(&mut self, id: NoteId) -> io::Result<()> {
        self.store.remove(id)?;
        self.notes.retain(|note| note.id != id);
        self.unsaved.remove(&id);
        Ok(())
    }

    /// The first note whose latest change the store could not keep, with why.
    pub(crate) fn unsaved(&self) -> Option<(&Note, &str)> {
        let (id, why) = self.unsaved.iter().next()?;
        Some((self.note(*id)?, why))
    }

    /// Tries again to keep each note the store could not.
    pub(crate) fn save_unsaved(&mut self) {
        let unsaved: Vec<NoteId> = self.unsaved.keys().copied().collect();
        for id in unsaved {
            self.keep(id);
        }
    }

    fn note_mut(&mut self, id: NoteId) -> Option<&mut Note> {
        self.notes.iter_mut().find(|note| note.id == id)
    }

    /// Writes the note to the store, and keeps why where it could not.
    fn keep(&mut self, id: NoteId) {
        let Some(note) = self.note(id) else {
            return;
        };
        match self.store.save(note) {
            Ok(()) => {
                self.unsaved.remove(&id);
            }
            Err(error) => {
                tracing::warn!(%id, %error, "a note is not kept");
                self.unsaved.insert(id, error.to_string());
            }
        }
    }

    /// Lets go for good of each note deleted longer ago than [`KEPT_DELETED`].
    fn remove_expired(&mut self, now: SystemTime) {
        let expired: Vec<NoteId> = self
            .notes
            .iter()
            .filter(|note| note.deleted.is_some_and(|at| at + KEPT_DELETED <= now))
            .map(|note| note.id)
            .collect();
        for id in expired {
            if let Err(error) = self.remove(id) {
                tracing::warn!(%id, %error, "a note deleted long ago is not let go");
            }
        }
    }

    /// Most recently changed first, and of two changed at once, the newer.
    fn sort(&mut self) {
        self.notes
            .sort_by(|a, b| b.changed.cmp(&a.changed).then(b.id.cmp(&a.id)));
    }

    /// An id no note has had: the milliseconds since the Unix epoch at `now`, or one past the
    /// newest id where that is not past it.
    fn next_id(&self, now: SystemTime) -> NoteId {
        let millis = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let from_now = u64::try_from(millis).unwrap_or(u64::MAX);
        let newest = self.notes.iter().map(|note| note.id.0).max();
        NoteId(newest.map_or(from_now, |newest| from_now.max(newest + 1)))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn library_in(folder: &Path, now: SystemTime) -> Library {
        Library::open(Store::at(folder.to_path_buf()), now)
    }

    fn titles(library: &Library) -> Vec<&str> {
        library.notes().iter().map(Note::title).collect()
    }

    #[test]
    fn notes_are_kept_as_they_change_and_listed_most_recently_changed_first() {
        let folder = tempfile::tempdir().unwrap();
        let mut library = library_in(folder.path(), at(100));
        let shopping = library.add("Shopping".to_owned(), at(100));
        library.add("Plans".to_owned(), at(200));
        assert_eq!(titles(&library), ["Plans", "Shopping"]);

        library.set_text(shopping, "Shopping\nmilk", at(300));
        assert_eq!(titles(&library), ["Shopping", "Plans"]);
        let reopened = library_in(folder.path(), at(400));
        assert_eq!(reopened.notes(), library.notes(), "what the store kept");
    }

    #[test]
    fn each_note_has_an_id_of_its_own_even_when_made_at_once() {
        let folder = tempfile::tempdir().unwrap();
        let mut library = library_in(folder.path(), at(100));
        let first = library.add(String::new(), at(100));
        let second = library.add(String::new(), at(100));
        assert_eq!(first, NoteId(100_000));
        assert_eq!(second, NoteId(100_001));
    }

    #[test]
    fn a_deleted_note_is_kept_until_recovered_or_thirty_days_have_passed() {
        let folder = tempfile::tempdir().unwrap();
        let mut library = library_in(folder.path(), at(100));
        let kept = library.add("Kept".to_owned(), at(100));
        let expired = library.add("Expired".to_owned(), at(100));
        library.delete(kept, at(200));
        library.delete(expired, at(200));
        library.recover(kept);
        assert!(!library.note(kept).unwrap().is_deleted());

        let month_on = at(200) + KEPT_DELETED;
        let reopened = library_in(folder.path(), month_on);
        assert_eq!(titles(&reopened), ["Kept"], "the other went for good");
    }

    #[test]
    fn a_note_the_store_cannot_keep_is_kept_once_it_can() {
        let folder = tempfile::tempdir().unwrap();
        let notes = folder.path().join("notes");
        let mut library = library_in(&notes, at(100));
        let shopping = library.add("Shopping".to_owned(), at(100));
        fs::remove_dir_all(&notes).unwrap();

        library.set_text(shopping, "Shopping\nmilk", at(200));
        let (unsaved, _why) = library.unsaved().expect("a note not kept");
        assert_eq!(unsaved.id, shopping);
        library.save_unsaved();
        assert!(library.unsaved().is_some(), "still no folder");
        fs::create_dir(&notes).unwrap();
        library.save_unsaved();
        assert!(library.unsaved().is_none());
    }

    #[test]
    fn a_note_removed_goes_for_good() {
        let folder = tempfile::tempdir().unwrap();
        let mut library = library_in(folder.path(), at(100));
        let shopping = library.add("Shopping".to_owned(), at(100));
        library.remove(shopping).unwrap();
        assert!(library.notes().is_empty());
        assert!(library_in(folder.path(), at(200)).notes().is_empty());
    }
}
