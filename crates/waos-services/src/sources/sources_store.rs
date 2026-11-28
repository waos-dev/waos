//! Where the list of sources is kept, in its own values ([`SourcesStore`]): a file today
//! (`sources_file`), the user's account later, memory in a test ([`SourcesInMemory`]).
//!
//! The invariant: a store keeps what it is handed and nothing else; every rule of the list (a
//! source added once, a list made once) is the sources' own.

use std::io;
use std::sync::{Mutex, PoisonError};

use crate::sources::Source;

/// Where the list of sources the user added is kept.
pub trait SourcesStore: Send + Sync {
    /// The sources kept, in the order added; none where no list was ever made, which an emptied
    /// list is not.
    fn load(&self) -> io::Result<Option<Vec<Source>>>;

    /// Keeps `sources` as the list, in place of what was kept.
    fn save(&self, sources: &[Source]) -> io::Result<()>;
}

/// A list kept in memory alone, gone with the value: for tests.
#[derive(Default)]
pub struct SourcesInMemory {
    sources: Mutex<Option<Vec<Source>>>,
}

impl SourcesStore for SourcesInMemory {
    fn load(&self) -> io::Result<Option<Vec<Source>>> {
        Ok(self
            .sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    fn save(&self, sources: &[Source]) -> io::Result<()> {
        *self.sources.lock().unwrap_or_else(PoisonError::into_inner) = Some(sources.to_vec());
        Ok(())
    }
}
