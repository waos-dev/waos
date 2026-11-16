//! The component-model streams a TCP socket is used through: its listener's accepted
//! connections, and its connection's bytes, read into and written from the program's memory in
//! place. A stream asked for nothing is asked whether it is ready; one cancelled while waiting
//! answers that it was; and a connection's stream answers how it ended through its call's
//! future.

use std::iter;
use std::marker::Unpin;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::BytesMut;
use tokio::sync::oneshot;
use wasmtime::component::{
    Destination, FutureReader, Resource, Source, StreamConsumer, StreamProducer, StreamReader,
    StreamResult,
};
use wasmtime::error::Context as _;
use wasmtime::{AsContextMut, StoreContextMut};

use super::NetworkCtx;
use super::bindings::wasi::sockets::types::ErrorCode;
use crate::network::sockets::{TcpListenStream, TcpReceiveStream, TcpSendStream, TcpSocket};

/// How much of a connection is read at a time where the program's buffer is larger.
const READ_LIMIT: usize = 8192;

/// How a connection's stream ended, as its call's future answers it.
pub(super) type StreamEnding = FutureReader<Result<(), ErrorCode>>;

/// A listener's accepted connections, each pushed into the program's table as it is handed over.
pub(super) struct ListenStreamProducer<F> {
    pub(super) listener: TcpListenStream,
    /// What reaches the program's table in its store.
    pub(super) getter: F,
}

impl<D, F> StreamProducer<D> for ListenStreamProducer<F>
where
    D: 'static,
    F: FnMut(&mut D) -> NetworkCtx<'_> + Send + Unpin + 'static,
{
    type Item = Resource<TcpSocket>;
    type Buffer = Option<Self::Item>;

    fn poll_produce<'a>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, D>,
        mut destination: Destination<'a, Self::Item, Self::Buffer>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        if destination.remaining(&mut store) == Some(0) {
            return match self.listener.poll_ready(cx) {
                Poll::Ready(()) => Poll::Ready(Ok(StreamResult::Completed)),
                Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
                Poll::Pending => Poll::Pending,
            };
        }
        let socket = match self.listener.poll_accept(cx) {
            Poll::Ready(socket) => socket,
            Poll::Pending if finish => return Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => return Poll::Pending,
        };
        let socket = (self.getter)(store.data_mut())
            .table
            .push(socket)
            .context("failed to push socket resource to table")?;
        destination.set_buffer(Some(socket));
        Poll::Ready(Ok(StreamResult::Completed))
    }
}

/// A connection's bytes as the program reads them, until the peer is done sending or the
/// connection fails, which its ending is told.
struct ReceiveStreamProducer(Option<(TcpReceiveStream, oneshot::Sender<Result<(), ErrorCode>>)>);

impl Drop for ReceiveStreamProducer {
    fn drop(&mut self) {
        self.close(Ok(()));
    }
}

impl ReceiveStreamProducer {
    fn close(&mut self, ending: Result<(), ErrorCode>) {
        if let Some((_, ended)) = self.0.take() {
            _ = ended.send(ending);
        }
    }
}

impl<D> StreamProducer<D> for ReceiveStreamProducer {
    type Item = u8;
    type Buffer = BytesMut;

    fn poll_produce<'a>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, D>,
        destination: Destination<'a, Self::Item, Self::Buffer>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let Some((stream, _)) = self.0.as_mut() else {
            return Poll::Ready(Ok(StreamResult::Dropped));
        };
        if destination.remaining(store.as_context_mut()) == Some(0) {
            return match stream.poll_ready(cx) {
                Poll::Ready(()) => Poll::Ready(Ok(StreamResult::Completed)),
                Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
                Poll::Pending => Poll::Pending,
            };
        }
        let mut destination = destination.as_direct(store, READ_LIMIT);
        match stream.poll_read(cx, destination.remaining()) {
            Poll::Ready(Ok(0)) => {
                self.close(Ok(()));
                Poll::Ready(Ok(StreamResult::Dropped))
            }
            Poll::Ready(Ok(read)) => {
                destination.mark_written(read);
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Poll::Ready(Err(error)) => {
                self.close(Err(error));
                Poll::Ready(Ok(StreamResult::Dropped))
            }
            Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// The program's bytes as a connection sends them, until the program ends its stream or the
/// connection fails, which its ending is told.
struct SendStreamConsumer(Option<(TcpSendStream, oneshot::Sender<Result<(), ErrorCode>>)>);

impl Drop for SendStreamConsumer {
    fn drop(&mut self) {
        self.close(Ok(()));
    }
}

impl SendStreamConsumer {
    fn close(&mut self, ending: Result<(), ErrorCode>) {
        if let Some((_, ended)) = self.0.take() {
            _ = ended.send(ending);
        }
    }
}

impl<D> StreamConsumer<D> for SendStreamConsumer {
    type Item = u8;

    fn poll_consume(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        source: Source<Self::Item>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let Some((stream, _)) = self.0.as_mut() else {
            return Poll::Ready(Ok(StreamResult::Dropped));
        };
        let mut source = source.as_direct(store);
        if source.remaining().is_empty() {
            return match stream.poll_ready(cx) {
                Poll::Ready(()) => Poll::Ready(Ok(StreamResult::Completed)),
                Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
                Poll::Pending => Poll::Pending,
            };
        }
        match stream.poll_write(cx, source.remaining()) {
            Poll::Ready(Ok(written)) => {
                debug_assert!(written > 0);
                source.mark_read(written);
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Poll::Ready(Err(error)) => {
                self.close(Err(error));
                Poll::Ready(Ok(StreamResult::Dropped))
            }
            Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Sends what the program writes into `data` through `stream`, and how that ended; a stream
/// not taken ends at once with its error.
pub(super) fn send(
    mut store: impl AsContextMut,
    stream: Result<TcpSendStream, ErrorCode>,
    mut data: StreamReader<u8>,
) -> wasmtime::Result<StreamEnding> {
    let mut store = store.as_context_mut();
    match stream {
        Ok(stream) => {
            let (ended, ending) = oneshot::channel();
            data.pipe(&mut store, SendStreamConsumer(Some((stream, ended))))?;
            FutureReader::new(&mut store, ending)
        }
        Err(error) => {
            data.close(&mut store)?;
            FutureReader::new(&mut store, async move { wasmtime::error::Ok(Err(error)) })
        }
    }
}

/// What the program reads of `stream`, and how that ended; a stream not taken is empty and ends
/// at once with its error.
pub(super) fn receive(
    mut store: impl AsContextMut,
    stream: Result<TcpReceiveStream, ErrorCode>,
) -> wasmtime::Result<(StreamReader<u8>, StreamEnding)> {
    let mut store = store.as_context_mut();
    match stream {
        Ok(stream) => {
            let (ended, ending) = oneshot::channel();
            Ok((
                StreamReader::new(&mut store, ReceiveStreamProducer(Some((stream, ended))))?,
                FutureReader::new(&mut store, ending)?,
            ))
        }
        Err(error) => Ok((
            StreamReader::new(&mut store, iter::empty())?,
            FutureReader::new(&mut store, async move { wasmtime::error::Ok(Err(error)) })?,
        )),
    }
}
