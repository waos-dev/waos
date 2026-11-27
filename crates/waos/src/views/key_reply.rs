//! A key's reply: whether the program handled it, which the shell waits on for each key it sends.
//! The systems want the answer before their key call returns, so the shell claims a key at once
//! and, once the reply says the program did not handle it, sends it on past the program, to its
//! menus' key equivalents.
//!
//! The invariant: every reply is answered exactly once, by the program or, dropped unanswered,
//! as not handled, so a shell waiting on one never waits for ever: dropped with the program's
//! store, with a view's events nobody read, or with the handle the program forgot.

use std::future::Future;
use std::pin::Pin;

use futures::FutureExt;
use futures::channel::oneshot;

/// What a key the shell sent answers: whether the program handled it.
pub type KeyHandledFuture = Pin<Box<dyn Future<Output = bool> + Send>>;

/// One key's reply, the value behind the program's `key-reply`.
pub struct KeyReply(Option<oneshot::Sender<bool>>);

impl KeyReply {
    /// A reply the shell waits on, and what it waits with.
    pub(super) fn awaited() -> (KeyReply, KeyHandledFuture) {
        let (reply, answer) = oneshot::channel();
        let handled = answer.map(|answer| answer.unwrap_or(false)).boxed();
        (KeyReply(Some(reply)), handled)
    }

    /// A reply nobody waits on: a synthesized key's.
    pub(super) fn unheard() -> KeyReply {
        KeyReply(None)
    }

    /// Answers whether the program handled the key; a second answer is ignored.
    pub(crate) fn answer(&mut self, handled: bool) {
        if let Some(reply) = self.0.take() {
            // A shell that stopped waiting has nothing to hear.
            let _ = reply.send(handled);
        }
    }
}

impl Drop for KeyReply {
    /// Unanswered, the key was not handled.
    fn drop(&mut self) {
        self.answer(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_answer_counts() {
        let (mut reply, handled) = KeyReply::awaited();
        reply.answer(true);
        reply.answer(false);
        assert_eq!(handled.now_or_never(), Some(true));
    }

    #[test]
    fn a_reply_dropped_unanswered_was_not_handled() {
        let (reply, handled) = KeyReply::awaited();
        drop(reply);
        assert_eq!(handled.now_or_never(), Some(false));
    }
}
