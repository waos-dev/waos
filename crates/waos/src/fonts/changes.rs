//! The installed fonts changing, as one program hears it: the manager's watch, made when the
//! program starts, counting each change onto every `changes` stream the program opened.
//!
//! The invariant: the count is of the changes since the program started, the same on every
//! stream, so a stream opened late, or read late, still says how many changes there have been.

use std::sync::{Arc, Mutex};

use futures::channel::mpsc;
use valo_fontmgr::{FontManager, Watch};

use crate::channel::Channel;
use crate::sync::locked;

/// One program's count of the installed fonts' changes, for as long as its watch lives.
pub(super) struct FontChanges {
    counts: Arc<Channel<u64>>,
    /// The manager's subscription, kept for the program's run and never touched; none when the
    /// program was given no fonts, or its manager cannot tell.
    _watch: Option<Watch>,
}

impl FontChanges {
    /// Counts the changes `manager` reports from now on.
    pub(super) fn watching(manager: Option<&Mutex<dyn FontManager + 'static>>) -> FontChanges {
        let counts = Arc::new(Channel::default());
        let watch = manager.and_then(|manager| {
            let counted = Arc::clone(&counts);
            let count = Mutex::new(0);
            super::lock(manager).watch(Box::new(move || {
                let mut count = locked(&count);
                *count += 1;
                // Sent under the lock, so two changes the platform reports from two threads
                // reach every stream in the order they were counted.
                counted.send(*count);
            }))
        });
        FontChanges {
            counts,
            _watch: watch,
        }
    }

    /// Each count from now on, on a stream of its own.
    pub(super) fn subscribe(&self) -> mpsc::UnboundedReceiver<u64> {
        self.counts.subscribe()
    }
}
