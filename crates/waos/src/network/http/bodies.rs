//! A response's body between hyper's client and `wasmtime-wasi-http`: read part by part within
//! the time the program allows between two, holding the request in flight until it ends.

use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use hyper::body::Incoming;
use tokio::time::Interval;
use wasmtime_wasi_http::Error;

use super::error_of;
use crate::access::ActiveUse;

/// A response's body, each part of which must come within a time of the one before, and its
/// request in flight until the body ends, fails or is let go.
pub(super) struct BetweenBytes {
    body: Incoming,
    deadline: Interval,
    in_flight: Option<ActiveUse>,
}

impl BetweenBytes {
    pub(super) fn new(body: Incoming, within: Duration, in_flight: ActiveUse) -> BetweenBytes {
        // An interval of zero is refused; the least there is waits next to nothing.
        let within = within.max(Duration::from_nanos(1));
        let mut deadline = tokio::time::interval(within);
        deadline.reset();
        BetweenBytes {
            body,
            deadline,
            in_flight: Some(in_flight),
        }
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
            Poll::Ready(Some(Err(error))) => {
                this.in_flight = None;
                Poll::Ready(Some(Err(error_of(&error))))
            }
            Poll::Ready(None) => {
                this.in_flight = None;
                Poll::Ready(None)
            }
            Poll::Pending => {
                ready!(this.deadline.poll_tick(cx));
                this.in_flight = None;
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
