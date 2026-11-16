//! A program started held, before its first instruction: its code is linked, so whoever listens
//! to its trace and to what it uses now hears it from its start, but it runs nothing, not even its
//! role's question, until its [`HeldProgram`] is dropped (decision 18 of
//! `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`, after Chrome DevTools
//! Protocol's `waitForDebuggerOnStart`).
//!
//! The invariant: dropping the [`HeldProgram`] is the one way a held program goes on, so whoever
//! held it cannot leave it held by ending; and its start is told as linked before it waits, so a
//! listener that subscribes once [`HeldProgram::linked`] resolves misses nothing.

use futures::channel::oneshot;

/// A program held before its first instruction, which runs once this is dropped.
pub struct HeldProgram {
    _release: oneshot::Sender<()>,
    linked: Option<oneshot::Receiver<()>>,
}

/// What a held program's start waits on.
pub(crate) struct Hold {
    released: oneshot::Receiver<()>,
    linked: oneshot::Sender<()>,
}

/// A hold for a program to start with, and the [`HeldProgram`] that lets it go.
pub(crate) fn hold() -> (HeldProgram, Hold) {
    let (release, released) = oneshot::channel();
    let (linked, told_linked) = oneshot::channel();
    (
        HeldProgram {
            _release: release,
            linked: Some(told_linked),
        },
        Hold { released, linked },
    )
}

impl HeldProgram {
    /// Resolves once the program's code is linked, so its trace and what it uses now can be
    /// listened to; at once where it ended before, or this was asked before.
    pub async fn linked(&mut self) {
        if let Some(linked) = self.linked.take() {
            // An error is a start that ended before it linked: nothing more to wait for.
            let _ = linked.await;
        }
    }
}

impl std::fmt::Debug for HeldProgram {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HeldProgram")
            .finish_non_exhaustive()
    }
}

impl Hold {
    /// Tells the program linked, then waits until its [`HeldProgram`] is dropped.
    pub(crate) async fn wait(self) {
        // Nobody waiting for the link is not an error.
        let _ = self.linked.send(());
        tracing::info!("held before its first instruction");
        // Dropped is the release; the channel carries nothing else.
        let _ = self.released.await;
        tracing::info!("let go");
    }
}
