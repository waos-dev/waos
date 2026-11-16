//! `wasdk:platform/file-watching`: what changes in a folder a program holds, as the system tells
//! it, through notify's watcher of each system: FSEvents on macOS; inotify on Linux and Android,
//! where notify watches each folder beneath a recursive watch itself, adding a watch for each new
//! one; and `ReadDirectoryChangesW` on Windows. A watch is of the folder's path on the machine,
//! which the files give only while it leads to the folder the program holds ([`HeldFolder`]),
//! checked again once the system watches it. Its events become the program's changes
//! ([`translation`]), gathered into batches ([`batch`]), each of which the program's stream takes
//! `latency` after the first change in it. A watch is a use of the program's files, decided and
//! told through its access, and held there while it lasts. The interface is served to a program
//! by `host`.
//!
//! A machine's root is a folder of this computer (`MachineFiles::Local`), watched as any other;
//! an SSH machine's files, once WaOS reaches one, come with a watch of their own.
//!
//! The invariant: a watch lives exactly as long as its stream, which holds the system's watcher
//! and the watch's use: the program dropping the stream ends the watch, and nothing else holds
//! it.

mod batch;
pub(crate) mod host;
mod translation;

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, BoxStream, StreamExt};
use notify::{Config, EventKindMask, RecommendedWatcher, RecursiveMode, Watcher};

use crate::access::ActiveUse;
use crate::bindings::wasdk::platform::file_watching::Change;
use crate::files::{ErrorCode, HeldFolder};
use batch::Arrivals;

/// A watch's batches of changes, as the program reads them; dropping it ends the watch.
pub(crate) type Batches = BoxStream<'static, Vec<Change>>;

/// Watches `folder`, its own entries, and everything under it where `recursive`, a symlink's
/// target never; each batch is taken `latency` after its first change arrived. The stream holds
/// `watching`, the watch's use, until it is dropped.
pub(crate) fn watch(
    folder: &HeldFolder,
    recursive: bool,
    latency: Duration,
    watching: ActiveUse,
) -> Result<Batches, ErrorCode> {
    let arrivals = Arc::new(Arrivals::new(folder.path().to_path_buf()));
    let heard = Arc::clone(&arrivals);
    let config = Config::default()
        .with_follow_symlinks(false)
        .with_event_kinds(EventKindMask::CORE);
    let mut watcher =
        RecommendedWatcher::new(move |event| heard.add(event), config).map_err(error_code_of)?;
    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    watcher.watch(folder.path(), mode).map_err(error_code_of)?;
    // The system resolved the path as it began watching; it must still have led to the folder.
    if !folder.is_at_its_path() {
        return Err(ErrorCode::NoEntry);
    }
    Ok(batches((watcher, watching), arrivals, latency))
}

/// The batches `watcher` gathers in `arrivals`, each taken `latency` after its first change; the
/// stream holds the watcher, and the watch's use.
fn batches(
    watcher: (RecommendedWatcher, ActiveUse),
    arrivals: Arc<Arrivals>,
    latency: Duration,
) -> Batches {
    stream::unfold((watcher, arrivals), move |(watcher, arrivals)| async move {
        loop {
            arrivals.something_arrived().await;
            if !latency.is_zero() {
                tokio::time::sleep(latency).await;
            }
            let batch = arrivals.take();
            if !batch.is_empty() {
                return Some((batch, (watcher, arrivals)));
            }
        }
    })
    .boxed()
}

/// What a watch that did not start fails with.
fn error_code_of(error: notify::Error) -> ErrorCode {
    tracing::debug!(%error, "a watch did not start");
    match error.kind {
        notify::ErrorKind::Io(error) => ErrorCode::from(error),
        notify::ErrorKind::PathNotFound | notify::ErrorKind::WatchNotFound => ErrorCode::NoEntry,
        // inotify's limit of watches, which Linux answers with `ENOSPC`.
        notify::ErrorKind::MaxFilesWatch => ErrorCode::InsufficientSpace,
        notify::ErrorKind::InvalidConfig(_) => ErrorCode::Invalid,
        _ => ErrorCode::Io,
    }
}

/// Changes compared whole, as the tests compare what a watch told.
#[cfg(test)]
impl PartialEq for Change {
    fn eq(&self, other: &Change) -> bool {
        match (self, other) {
            (Change::Created(one), Change::Created(other))
            | (Change::Modified(one), Change::Modified(other))
            | (Change::Removed(one), Change::Removed(other))
            | (Change::Other(one), Change::Other(other)) => one == other,
            (Change::Renamed(one), Change::Renamed(other)) => one == other,
            (Change::Rescan(one), Change::Rescan(other)) => one == other,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
