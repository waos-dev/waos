//! Where the list of sources and the pictures read from them are kept, in their own values
//! ([`SourcesStore`]): files today (`sources_file`), the user's account later, memory in a test
//! ([`SourcesInMemory`]).
//!
//! The invariant: a store keeps what it is handed and nothing else; every rule of the list (a
//! source added once, a list made once) is the sources' own.

use std::collections::HashMap;
use std::io;
use std::sync::{Mutex, MutexGuard, PoisonError};

use url::Url;

use crate::sources::Source;

/// Where the list of sources the user added is kept, and the pictures their indexes list, each
/// under its address.
pub trait SourcesStore: Send + Sync {
    /// The sources kept, in the order added; none where no list was ever made, which an emptied
    /// list is not.
    fn load(&self) -> io::Result<Option<Vec<Source>>>;

    /// Keeps `sources` as the list, in place of what was kept.
    fn save(&self, sources: &[Source]) -> io::Result<()>;

    /// The picture kept for the address `url`; none where none is kept.
    fn picture(&self, url: &Url) -> io::Result<Option<Vec<u8>>>;

    /// Keeps `bytes` as the picture at `url`, in place of what was kept for it.
    fn keep_picture(&self, url: &Url, bytes: &[u8]) -> io::Result<()>;

    /// Removes every picture kept for an address none of `listed`.
    fn retain_pictures(&self, listed: &[Url]) -> io::Result<()>;
}

/// A list and its pictures kept in memory alone, gone with the value: for tests.
#[derive(Default)]
pub struct SourcesInMemory {
    sources: Mutex<Option<Vec<Source>>>,
    pictures: Mutex<HashMap<Url, Vec<u8>>>,
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

    fn picture(&self, url: &Url) -> io::Result<Option<Vec<u8>>> {
        Ok(self.locked_pictures().get(url).cloned())
    }

    fn keep_picture(&self, url: &Url, bytes: &[u8]) -> io::Result<()> {
        self.locked_pictures().insert(url.clone(), bytes.to_vec());
        Ok(())
    }

    fn retain_pictures(&self, listed: &[Url]) -> io::Result<()> {
        self.locked_pictures().retain(|url, _| listed.contains(url));
        Ok(())
    }
}

impl SourcesInMemory {
    fn locked_pictures(&self) -> MutexGuard<'_, HashMap<Url, Vec<u8>>> {
        // Every change leaves the map whole, so one that panicked leaves nothing half-changed.
        self.pictures.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        text.parse().unwrap()
    }

    #[test]
    fn a_picture_is_kept_until_no_listed_address_is_its_own() {
        let store = SourcesInMemory::default();
        let (icon, screenshot) = (
            url("https://example.com/icon.png"),
            url("https://example.com/1.png"),
        );
        store.keep_picture(&icon, b"an icon").unwrap();
        store.keep_picture(&screenshot, b"a screenshot").unwrap();
        store.retain_pictures(std::slice::from_ref(&icon)).unwrap();
        assert_eq!(
            store.picture(&icon).unwrap().as_deref(),
            Some(&b"an icon"[..])
        );
        assert_eq!(store.picture(&screenshot).unwrap(), None);
    }
}
