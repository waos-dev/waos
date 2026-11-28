//! [`Sources`]: the list of sources the user added, kept in the store it is handed, and what is
//! read from a source: its index, its pictures, kept in the store too, and its packages'
//! downloads.
//!
//! The invariant a later editor will break: every change to the list holds one lock from its read
//! to its write, so two edits never interleave; and a source's listing of a development id, which
//! no package takes, is refused before its download.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use reqwest::StatusCode;
use url::Url;
use waos_package::{Index, ListedProgram};

use crate::sources::fetch::{self, Download, SourceProxy};
use crate::sources::{Source, SourceError, SourceListError, SourcesStore};

/// The sources the user added, and what is read from them. Clones are the same.
#[derive(Clone)]
pub struct Sources {
    shared: Arc<Shared>,
}

struct Shared {
    store: Arc<dyn SourcesStore>,
    http: reqwest::Client,
    /// Held while the list changes, so changes happen one at a time.
    changes: Mutex<()>,
}

impl Sources {
    /// The sources kept in `store`, with their pictures, read through `proxy`.
    pub fn open(store: Arc<dyn SourcesStore>, proxy: SourceProxy) -> Result<Sources, SourceError> {
        Ok(Sources {
            shared: Arc::new(Shared {
                store,
                http: fetch::http_client(proxy)?,
                changes: Mutex::default(),
            }),
        })
    }

    /// The sources the user added, in the order added; none when they cannot be read, which is
    /// said to whoever subscribes.
    pub fn list(&self) -> Vec<Source> {
        self.shared.store.load().map_or_else(
            |error| {
                tracing::warn!("the sources cannot be read: {error}");
                Vec::new()
            },
            Option::unwrap_or_default,
        )
    }

    /// Makes the list `first` where none was ever made, as WaOS comes with sources of its own. A
    /// list made already is kept as it is, emptied or not, so a source the user removed never
    /// comes back.
    pub fn start_with(&self, first: &[Source]) -> Result<(), SourceListError> {
        let _changing = self.lock_changes();
        if self.shared.store.load()?.is_none() {
            self.shared.store.save(first)?;
        }
        Ok(())
    }

    /// Adds `source` after the others; a source added already stays where it is.
    pub fn add(&self, source: Source) -> Result<(), SourceListError> {
        let _changing = self.lock_changes();
        let mut sources = self.shared.store.load()?.unwrap_or_default();
        if !sources.contains(&source) {
            sources.push(source);
            self.shared.store.save(&sources)?;
        }
        Ok(())
    }

    /// Removes `source` from the list; one not among them is refused.
    pub fn remove(&self, source: &Source) -> Result<(), SourceListError> {
        let _changing = self.lock_changes();
        let mut sources = self.shared.store.load()?.unwrap_or_default();
        let count = sources.len();
        sources.retain(|added| added != source);
        if sources.len() == count {
            return Err(SourceListError::NotAdded(source.clone()));
        }
        Ok(self.shared.store.save(&sources)?)
    }

    /// The index `source` serves: the programs it offers, each at a version, with its package.
    /// An address that answers but has no index is told apart from one that could not be read:
    /// it is no source, as a website that lists no apps.
    pub async fn read_index(&self, source: &Source) -> Result<Index, SourceError> {
        let read = fetch::read_index(&self.shared.http, &source.index_url()).await;
        read.map_err(|error| match error {
            SourceError::Status { url, status } if status == StatusCode::NOT_FOUND => {
                SourceError::ListsNothing { url }
            }
            other => other,
        })
    }

    /// Starts downloading `program` as `source` lists it: its package's bytes as they arrive,
    /// for the package manager to take in, ending in an error unless their SHA-256 is the one
    /// listed. A listing of a development install's id, which no package may take, is refused
    /// before anything is downloaded.
    pub async fn download(
        &self,
        source: &Source,
        program: &ListedProgram,
    ) -> Result<Download, SourceError> {
        if program.id.is_development() {
            return Err(SourceError::DevelopmentId(program.id.clone()));
        }
        let url = source.package_url(&program.package)?;
        fetch::download(&self.shared.http, &url, &program.sha256).await
    }

    /// The picture `source`'s index lists as `picture`, an icon or a screenshot, as its file
    /// holds it: read from the source's own server the first time and kept in the store, by its
    /// address, from then on, so a store shows it at once and offline. A picture's address names
    /// its app's version, so a new version's is another picture.
    pub async fn picture(&self, source: &Source, picture: &str) -> Result<Vec<u8>, SourceError> {
        let url = source.picture_url(picture)?;
        match self.shared.store.picture(&url) {
            Ok(Some(bytes)) => return Ok(bytes),
            Ok(None) => {}
            Err(error) => tracing::warn!(%url, %error, "a source's kept picture cannot be read"),
        }
        let bytes = fetch::read_picture(&self.shared.http, &url).await?;
        // A picture that cannot be kept is still shown; it is read again next time.
        if let Err(error) = self.shared.store.keep_picture(&url, &bytes) {
            tracing::warn!(%url, %error, "a source's picture is not kept");
        }
        Ok(bytes)
    }

    /// Removes the pictures kept that are none of `listed`, the addresses the sources read now
    /// list: an app's older version's, and a removed source's.
    pub fn retain_pictures(&self, listed: &[Url]) {
        if let Err(error) = self.shared.store.retain_pictures(listed) {
            tracing::warn!(%error, "a source's picture no longer listed stays");
        }
    }

    fn lock_changes(&self) -> MutexGuard<'_, ()> {
        // The lock guards no value, so a change that panicked leaves nothing half-changed in it.
        self.shared
            .changes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}
