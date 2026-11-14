//! A value the shell reports and the engine keeps the newest of, a player's state or a capture
//! session's, as the stream of its changes a program reads: each read takes the newest value,
//! passing over those reported between.
//!
//! The invariant: a stream holds at most the one value it has not read, and ends once the value's
//! sender is gone, so a stream outlives nothing it watches.

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use tokio::sync::watch;

/// Each value `changes` holds from its next change on, the newest at each read; the stream ends
/// once the value's sender is gone.
pub(crate) fn changes_of<T: Clone + Send + Sync + 'static>(
    changes: watch::Receiver<T>,
) -> BoxStream<'static, T> {
    stream::unfold(changes, |mut changes| async move {
        changes.changed().await.ok()?;
        let value = changes.borrow_and_update().clone();
        Some((value, changes))
    })
    .boxed()
}
