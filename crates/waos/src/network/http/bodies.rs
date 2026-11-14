//! A request's and a response's bodies between `wasmtime-wasi-http` and reqwest: the program's
//! request body as reqwest takes one, and the response's body read part by part within the time
//! the program allows between two.

use std::pin::Pin;
use std::sync::{Mutex, PoisonError};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use http_body::{Body, Frame, SizeHint};
use tokio::time::Interval;
use wasmtime_wasi_http::{Error, WasiBody};
use wasmtime_wasi_io::bytes::Bytes;

use super::error_of;

/// The program's request body, which reqwest takes only as `Sync`, which a body wrapped in a
/// lock is: reading it is through `&mut`, and never locks.
pub(super) struct SyncBody(Mutex<WasiBody>);

impl SyncBody {
    pub(super) fn new(body: WasiBody) -> SyncBody {
        SyncBody(Mutex::new(body))
    }

    /// The body, through `&mut`, which no other holder of the lock can have.
    fn body(&mut self) -> &mut WasiBody {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Body for SyncBody {
    type Data = Bytes;
    type Error = Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(self.get_mut().body()).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .size_hint()
    }
}

/// A response's body, each part of which must come within a time of the one before.
pub(super) struct BetweenBytes {
    body: reqwest::Body,
    deadline: Interval,
}

impl BetweenBytes {
    pub(super) fn new(body: reqwest::Body, within: Duration) -> BetweenBytes {
        // An interval of zero is refused; the least there is waits next to nothing.
        let within = within.max(Duration::from_nanos(1));
        let mut deadline = tokio::time::interval(within);
        deadline.reset();
        BetweenBytes { body, deadline }
    }
}

impl Body for BetweenBytes {
    type Data = Bytes;
    type Error = Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        match Pin::new(&mut this.body).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                this.deadline.reset();
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error_of(&error)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => {
                ready!(this.deadline.poll_tick(cx));
                Poll::Ready(Some(Err(Error::ConnectionReadTimeout)))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}
