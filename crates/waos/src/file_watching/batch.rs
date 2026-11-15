//! The changes a watch has heard and the program has not read yet: added on the system's watcher's
//! thread as each event arrives, taken whole by the program's stream as one batch.
//!
//! A rescan of the whole folder, the system's after it dropped events or WaOS's, makes every other
//! change of its batch moot: the batch is that rescan alone until it is taken.
//!
//! The invariant: a batch holds at most [`LIMIT`] changes; past that its changes are dropped and
//! one rescan of the whole folder stands for them, so a program that stops reading costs WaOS a
//! bounded amount of memory and still learns that it must read the folder again.

use std::path::PathBuf;
use std::sync::Mutex;

use notify::Event;
use tokio::sync::Notify;

use super::translation::Translation;
use crate::bindings::wasdk::platform::file_watching::Change;
use crate::sync::locked;

/// The most changes one batch holds.
pub(super) const LIMIT: usize = 4096;

/// The changes heard and not read, and what wakes the stream that reads them.
pub(super) struct Arrivals {
    batch: Mutex<Batch>,
    arrived: Notify,
}

/// The changes of the batch being gathered.
struct Batch {
    translation: Translation,
    changes: Vec<Change>,
    /// Whether the batch is one rescan of the whole folder, which later changes add nothing to.
    rescans_all: bool,
}

impl Arrivals {
    /// The arrivals of a watch of `folder`, as the system names its paths.
    pub(super) fn new(folder: PathBuf) -> Arrivals {
        Arrivals {
            batch: Mutex::new(Batch {
                translation: Translation::new(folder),
                changes: Vec::new(),
                rescans_all: false,
            }),
            arrived: Notify::new(),
        }
    }

    /// Adds the changes `event` makes, and wakes the stream.
    pub(super) fn add(&self, event: notify::Result<Event>) {
        locked(&self.batch).add(event);
        self.arrived.notify_one();
    }

    /// Waits until something has arrived since the last batch was taken.
    pub(super) async fn something_arrived(&self) {
        loop {
            if !locked(&self.batch).is_empty() {
                return;
            }
            self.arrived.notified().await;
        }
    }

    /// The batch gathered, emptied for the next: the end of a rename held, which nothing has
    /// paired by now, is its path's removal.
    pub(super) fn take(&self) -> Vec<Change> {
        locked(&self.batch).take()
    }
}

impl Batch {
    fn add(&mut self, event: notify::Result<Event>) {
        if self.rescans_all {
            return;
        }
        let changes = self.translation.changes_of(event);
        self.extend(changes);
    }

    /// Adds `changes`, or makes the batch one rescan of the whole folder where they hold one or
    /// would take it past its limit.
    fn extend(&mut self, changes: Vec<Change>) {
        if changes
            .iter()
            .any(|change| matches!(change, Change::Rescan(None)))
        {
            self.rescan_all();
        } else if self.changes.len() + changes.len() > LIMIT {
            tracing::debug!("a watch's batch overflowed: the program reads the folder again");
            self.rescan_all();
        } else {
            self.changes.extend(changes);
        }
    }

    /// Drops the changes gathered for one rescan of the whole folder.
    fn rescan_all(&mut self) {
        self.rescans_all = true;
        self.changes = vec![Change::Rescan(None)];
        // What a rescan reads again holds the rename's end too.
        let _ = self.translation.held_from_unpaired();
    }

    fn is_empty(&self) -> bool {
        self.changes.is_empty() && !self.translation.holds_from()
    }

    fn take(&mut self) -> Vec<Change> {
        if !self.rescans_all {
            let unpaired = self.translation.held_from_unpaired();
            self.extend(unpaired);
        }
        self.rescans_all = false;
        std::mem::take(&mut self.changes)
    }
}

#[cfg(test)]
mod tests {
    use notify::EventKind;
    use notify::event::{CreateKind, ModifyKind, RenameMode};

    use super::*;

    fn created(name: &str) -> notify::Result<Event> {
        Ok(
            Event::new(EventKind::Create(CreateKind::File))
                .add_path(PathBuf::from("/w").join(name)),
        )
    }

    #[test]
    fn a_batch_is_what_arrived_since_the_last_and_a_rename_unpaired_by_then_a_removal() {
        let arrivals = Arrivals::new(PathBuf::from("/w"));
        arrivals.add(created("a"));
        arrivals.add(Ok(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::From,
        )))
        .add_path(PathBuf::from("/w/b"))));
        assert_eq!(
            arrivals.take(),
            [
                Change::Created("a".to_owned()),
                Change::Removed("b".to_owned())
            ]
        );
        assert!(arrivals.take().is_empty());
    }

    #[test]
    fn a_rescan_of_everything_is_the_whole_batch() {
        let arrivals = Arrivals::new(PathBuf::from("/w"));
        arrivals.add(created("before"));
        arrivals.add(Ok(
            Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan)
        ));
        arrivals.add(created("after"));
        assert_eq!(arrivals.take(), [Change::Rescan(None)]);
    }

    #[test]
    fn a_batch_past_its_limit_is_one_rescan_and_the_next_starts_afresh() {
        let arrivals = Arrivals::new(PathBuf::from("/w"));
        for index in 0..=LIMIT {
            arrivals.add(created(&index.to_string()));
        }
        assert_eq!(arrivals.take(), [Change::Rescan(None)]);
        arrivals.add(created("after"));
        assert_eq!(arrivals.take(), [Change::Created("after".to_owned())]);
    }
}
