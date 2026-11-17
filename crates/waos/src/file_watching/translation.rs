//! The system's events, as notify reports them, turned into the program's changes: each path made
//! relative to the watched folder, its names joined by `/`, the folder itself `.`; the two ends of
//! a rename paired where the system pairs them; and lost events, the system's overflow or a watch
//! that failed, a rescan.
//!
//! notify tells the two ends of a rename apart, in the order they happened: inotify's as the
//! `From`, then the `To` and a `Both` pairing them under one cookie, Windows' as a `From` and the
//! `To` that follows it, FSEvents' as `Any`, each an end the system did not pair, which is a
//! `moved`: the path may be the one it left or the one it arrived at. A `From` is held until what
//! follows says whether it is paired; a `From` nothing pairs is the path's removal, a `To` nothing
//! pairs its creation.
//!
//! The invariant: no change names a path outside the watched folder; an end of a rename outside
//! it is a removal or a creation inside it.

use std::path::{Component, Path, PathBuf};

use notify::Event;
use notify::event::{EventKind, Flag, ModifyKind, RenameMode};

use crate::bindings::wasdk::platform::file_watching::Change;

/// The program's name for the watched folder itself.
const FOLDER_ITSELF: &str = ".";

/// One watch's events turned into its changes.
pub(super) struct Translation {
    /// The watched folder, as the system names the paths of its events.
    folder: PathBuf,
    /// The `From` of a rename, until what follows says whether it is paired: its path, and the
    /// cookie that pairs it, where the system gives one.
    held_from: Option<(PathBuf, Option<usize>)>,
    /// The cookie of the rename paired last, whose `Both` says nothing new.
    paired: Option<usize>,
}

impl Translation {
    /// The translation of the events of `folder`.
    pub(super) fn new(folder: PathBuf) -> Translation {
        Translation {
            folder,
            held_from: None,
            paired: None,
        }
    }

    /// The changes `event` makes, or a rescan of what a failed watch lost, after the end of a
    /// rename held that it does not pair.
    pub(super) fn changes_of(&mut self, event: notify::Result<Event>) -> Vec<Change> {
        let event = match event {
            Ok(event) => event,
            Err(error) => return self.lost(&error.paths, &error.to_string()),
        };
        if event.flag() == Some(Flag::Rescan) {
            return self.lost(&event.paths, "the system dropped events");
        }
        let tracker = event.tracker();
        match (event.kind, event.paths.as_slice()) {
            (EventKind::Modify(ModifyKind::Name(RenameMode::From)), [from]) => {
                let changes = self.held_from_unpaired();
                self.held_from = Some((from.clone(), tracker));
                changes
            }
            (EventKind::Modify(ModifyKind::Name(RenameMode::To)), [to]) => self.to(to, tracker),
            (EventKind::Modify(ModifyKind::Name(RenameMode::Both)), [from, to]) => {
                if tracker.is_some() && tracker == self.paired {
                    return self.held_from_unpaired();
                }
                let mut changes = self.held_from_unpaired();
                changes.extend(self.renamed(from, to));
                changes
            }
            (kind, paths) => {
                let mut changes = self.held_from_unpaired();
                changes.extend(paths.iter().filter_map(|path| self.change(&kind, path)));
                changes
            }
        }
    }

    /// Whether the `From` of a rename is held, waiting for what pairs it.
    pub(super) fn holds_from(&self) -> bool {
        self.held_from.is_some()
    }

    /// The removal of the `From` held, which nothing has paired; none where none is held.
    pub(super) fn held_from_unpaired(&mut self) -> Vec<Change> {
        self.held_from
            .take()
            .and_then(|(from, _)| self.named(&from, Change::Removed))
            .into_iter()
            .collect()
    }

    /// The `To` of a rename: paired with the `From` held, where their cookies agree or neither
    /// has one; else the creation of `to`.
    fn to(&mut self, to: &Path, tracker: Option<usize>) -> Vec<Change> {
        match self.held_from.take() {
            Some((from, from_tracker)) if from_tracker == tracker => {
                self.paired = tracker;
                self.renamed(&from, to)
            }
            unpaired => {
                let removed = unpaired.and_then(|(from, _)| self.named(&from, Change::Removed));
                removed
                    .into_iter()
                    .chain(self.named(to, Change::Created))
                    .collect()
            }
        }
    }

    /// The rename of `from` to `to`, either end of which may be outside the folder, or named
    /// otherwise than in text.
    fn renamed(&self, from: &Path, to: &Path) -> Vec<Change> {
        match (self.relative(from), self.relative(to)) {
            (Some(Some(from)), Some(Some(to))) => vec![Change::Renamed((from, to))],
            _ => [
                self.named(from, Change::Removed),
                self.named(to, Change::Created),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }

    /// What an event of `kind` at `path` is, but for a rename's ends; none for a read, which
    /// changes nothing, and a path outside the folder.
    fn change(&self, kind: &EventKind, path: &Path) -> Option<Change> {
        let change: fn(String) -> Change = match kind {
            EventKind::Access(_) => return None,
            EventKind::Create(_) => Change::Created,
            EventKind::Remove(_) => Change::Removed,
            EventKind::Modify(ModifyKind::Name(RenameMode::Any)) => Change::Moved,
            EventKind::Modify(ModifyKind::Name(_)) | EventKind::Any | EventKind::Other => {
                Change::Other
            }
            EventKind::Modify(_) => Change::Modified,
        };
        self.named(path, change)
    }

    /// `change` of `path`, by its path relative to the folder; for a path whose names are not all
    /// text, a rescan of the nearest folder above it whose are, which the program reads again for
    /// it; none outside the folder.
    fn named(&self, path: &Path, change: fn(String) -> Change) -> Option<Change> {
        Some(match self.relative(path)? {
            Some(path) => change(path),
            None => Change::Rescan(self.nearest_named(path)),
        })
    }

    /// A rescan of what `paths` lost, or of the whole folder where they name none of it or the
    /// folder itself; told in the log with `why`.
    fn lost(&mut self, paths: &[PathBuf], why: &str) -> Vec<Change> {
        tracing::debug!(folder = %self.folder.display(), "a watch lost changes: {why}");
        let mut changes = self.held_from_unpaired();
        changes.push(Change::Rescan(
            paths.first().and_then(|path| self.nearest_named(path)),
        ));
        changes
    }

    /// `path` relative to the folder, its names joined by `/`, `.` for the folder itself, or none
    /// where a name is not text; none outside the folder.
    fn relative(&self, path: &Path) -> Option<Option<String>> {
        let within = path.strip_prefix(&self.folder).ok()?;
        Some(names_of(within))
    }

    /// What a rescan for `path` reads again: the nearest folder at or above it whose names are all
    /// text, relative to the folder; none for the folder itself, or a path outside it.
    fn nearest_named(&self, path: &Path) -> Option<String> {
        let within = path.strip_prefix(&self.folder).ok()?;
        within
            .ancestors()
            .find_map(names_of)
            .filter(|path| path != FOLDER_ITSELF)
    }
}

/// The names of `within`, a path inside the folder, joined by `/`; `.` for none; none where a
/// name is not text.
fn names_of(within: &Path) -> Option<String> {
    let names: Option<Vec<&str>> = within
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_str()),
            _ => None,
        })
        .collect();
    let names = names?;
    Some(if names.is_empty() {
        FOLDER_ITSELF.to_owned()
    } else {
        names.join("/")
    })
}

#[cfg(test)]
mod tests {
    use notify::event::{CreateKind, DataChange, RemoveKind};

    use super::*;

    const FOLDER: &str = "/watched";

    fn at(name: &str) -> PathBuf {
        Path::new(FOLDER).join(name)
    }

    fn event(kind: EventKind, paths: &[&str]) -> notify::Result<Event> {
        let mut event = Event::new(kind);
        for path in paths {
            event = event.add_path(at(path));
        }
        Ok(event)
    }

    fn rename(mode: RenameMode, paths: &[&str], cookie: Option<usize>) -> notify::Result<Event> {
        let event = event(EventKind::Modify(ModifyKind::Name(mode)), paths)?;
        Ok(match cookie {
            Some(cookie) => event.set_tracker(cookie),
            None => event,
        })
    }

    fn translated(events: Vec<notify::Result<Event>>) -> Vec<Change> {
        let mut translation = Translation::new(PathBuf::from(FOLDER));
        let mut changes: Vec<Change> = events
            .into_iter()
            .flat_map(|event| translation.changes_of(event))
            .collect();
        changes.extend(translation.held_from_unpaired());
        changes
    }

    fn renamed(from: &str, to: &str) -> Change {
        Change::Renamed((from.to_owned(), to.to_owned()))
    }

    #[test]
    fn a_change_names_its_path_relative_to_the_folder_and_a_read_is_none() {
        let changes = translated(vec![
            event(EventKind::Create(CreateKind::File), &["a.txt"]),
            event(
                EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                &["sub/b.txt"],
            ),
            event(EventKind::Remove(RemoveKind::Folder), &["sub"]),
            event(
                EventKind::Access(notify::event::AccessKind::Any),
                &["a.txt"],
            ),
            event(EventKind::Any, &[""]),
        ]);
        assert_eq!(
            changes,
            [
                Change::Created("a.txt".to_owned()),
                Change::Modified("sub/b.txt".to_owned()),
                Change::Removed("sub".to_owned()),
                Change::Other(".".to_owned()),
            ]
        );
    }

    #[test]
    fn inotify_s_rename_is_one_rename_and_windows_from_and_to_are_paired() {
        let inotify = translated(vec![
            rename(RenameMode::From, &["a"], Some(7)),
            rename(RenameMode::To, &["b"], Some(7)),
            rename(RenameMode::Both, &["a", "b"], Some(7)),
        ]);
        assert_eq!(inotify, [renamed("a", "b")]);
        let windows = translated(vec![
            rename(RenameMode::From, &["a"], None),
            rename(RenameMode::To, &["b"], None),
        ]);
        assert_eq!(windows, [renamed("a", "b")]);
    }

    #[test]
    fn an_end_of_a_rename_nothing_pairs_is_a_removal_a_creation_or_fsevents_moved() {
        let changes = translated(vec![
            rename(RenameMode::From, &["gone"], Some(1)),
            event(EventKind::Create(CreateKind::File), &["new"]),
            rename(RenameMode::To, &["arrived"], Some(2)),
            rename(RenameMode::Any, &["left"], None),
            rename(RenameMode::Any, &["arrived-at"], None),
            rename(RenameMode::From, &["last"], Some(3)),
        ]);
        assert_eq!(
            changes,
            [
                Change::Removed("gone".to_owned()),
                Change::Created("new".to_owned()),
                Change::Created("arrived".to_owned()),
                Change::Moved("left".to_owned()),
                Change::Moved("arrived-at".to_owned()),
                Change::Removed("last".to_owned()),
            ]
        );
    }

    #[test]
    fn a_rename_out_of_the_folder_is_a_removal_and_into_it_a_creation() {
        let mut translation = Translation::new(PathBuf::from(FOLDER));
        let both = |from: &str, to: &str| {
            Ok(
                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                    .add_path(PathBuf::from(from))
                    .add_path(PathBuf::from(to)),
            )
        };
        assert_eq!(
            translation.changes_of(both("/watched/a", "/elsewhere/a")),
            [Change::Removed("a".to_owned())]
        );
        assert_eq!(
            translation.changes_of(both("/elsewhere/b", "/watched/b")),
            [Change::Created("b".to_owned())]
        );
        assert!(
            translation
                .changes_of(
                    event(EventKind::Create(CreateKind::File), &[])
                        .map(|event| event.add_path(PathBuf::from("/elsewhere/c")))
                )
                .is_empty(),
            "nothing outside the folder"
        );
    }

    #[test]
    fn lost_events_are_a_rescan_of_their_path_or_of_everything() {
        let overflow = || Ok(Event::new(EventKind::Other).set_flag(Flag::Rescan));
        let under = || overflow().map(|event| event.add_path(at("sub")));
        let of_folder = || overflow().map(|event| event.add_path(at("")));
        let failed = || Err(notify::Error::generic("the watch failed").add_path(at("deep/er")));
        assert_eq!(
            translated(vec![overflow(), under(), of_folder(), failed()]),
            [
                Change::Rescan(None),
                Change::Rescan(Some("sub".to_owned())),
                Change::Rescan(None),
                Change::Rescan(Some("deep/er".to_owned())),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_text_is_a_rescan_of_the_folder_above_it() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;

        let mut translation = Translation::new(PathBuf::from(FOLDER));
        let path = at("sub").join(OsStr::from_bytes(b"\xff"));
        let event = Event::new(EventKind::Create(CreateKind::File)).add_path(path);
        assert_eq!(
            translation.changes_of(Ok(event)),
            [Change::Rescan(Some("sub".to_owned()))]
        );
    }
}
