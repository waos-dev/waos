//! A screen the shell shows before its program asks for a view, such as a phone's tab or a card
//! Android restored, kept for the program's first `request-view`: the request is answered with it
//! at once, and the shell hears which view it became, to show the program in it from then on.
//!
//! The invariant: a reserved screen answers one request at most, the first, and a shell waiting
//! on it hears either that view or, once the program ends without asking, that it never will.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{self, BoxFuture, Either};

use super::ProgramView;
use crate::running::ProgramEnd;
use crate::shell::PlacedView;

/// The screen kept, and where the view it becomes is sent.
pub(super) struct Reserved {
    placed: PlacedView,
    taken: oneshot::Sender<ProgramView>,
}

impl Reserved {
    /// `placed` kept, with what hears the view it becomes.
    pub(super) fn new(placed: PlacedView) -> (Reserved, oneshot::Receiver<ProgramView>) {
        let (taken, heard) = oneshot::channel();
        (Reserved { placed, taken }, heard)
    }

    pub(super) fn into_parts(self) -> (PlacedView, oneshot::Sender<ProgramView>) {
        (self.placed, self.taken)
    }
}

/// What the shell waits on for a screen it reserved for a program
/// ([`WaOS::run_reserving`](crate::WaOS::run_reserving)): the view the screen became, once the
/// program's first `request-view` takes it, or none once the program ends without asking.
pub struct ReservedView(BoxFuture<'static, Option<ProgramView>>);

impl ReservedView {
    /// Resolves with what `taken` hears, or with none once `ended` resolves first.
    pub(crate) fn new(
        taken: oneshot::Receiver<ProgramView>,
        ended: impl Future<Output = ProgramEnd> + Send + 'static,
    ) -> ReservedView {
        ReservedView(
            async move {
                match future::select(taken, Box::pin(ended)).await {
                    Either::Left((taken, _)) => taken.ok(),
                    Either::Right(_) => None,
                }
            }
            .boxed(),
        )
    }
}

impl Future for ReservedView {
    type Output = Option<ProgramView>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.poll_unpin(cx)
    }
}
