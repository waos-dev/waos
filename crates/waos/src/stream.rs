//! A program's streams and futures: one of the engine's `futures` streams, delivered to the
//! program one item per read as wasmtime's producer, each item turned into the program's on its
//! way, which may need the store: a view becomes a handle on it in the store's table. Or each item
//! with what is held until the program has handled it (`stream_holding`, `stream_of_holding`). And bytes
//! both ways: bytes the engine has, as the program's stream (`stream_of_bytes`), or shares with
//! others, a chunk at a time (`stream_of_shared_bytes`), or as they come
//! in chunks (`stream_of_chunks`), a stream of bytes the program gives, collected until it ends (`Collected`) or passed on as it comes (`Chunks`),
//! the items of any stream the program gives, passed on one at a time (`Forwarded`), and the value
//! a future the program gives settles with (`Settled`).
//!
//! The invariant: an item is taken from the source only when the program reads, so nothing is
//! read ahead that a program closing the stream would lose; what is held for an item is let go
//! only once the program reads again or drops the stream; and bytes the program gives are taken
//! only as fast as whoever they are passed on to takes them.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::channel::{mpsc, oneshot};
use futures::{Stream, StreamExt};
use wasmtime::StoreContextMut;
use wasmtime::component::{
    Access, Destination, FutureConsumer, HasData, Lift, Lower, Source, StreamConsumer,
    StreamProducer, StreamReader, StreamResult, VecBuffer,
};

/// `source`, as the program's stream, each item converted by `convert` with the store's data.
pub(crate) fn stream_of<D, A, S, F, Out>(
    access: Access<'_, D, A>,
    source: S,
    convert: F,
) -> wasmtime::Result<StreamReader<Out>>
where
    D: 'static,
    A: HasData + ?Sized,
    S: Stream + Send + Unpin + 'static,
    F: FnMut(&mut D, S::Item) -> wasmtime::Result<Out> + Send + Unpin + 'static,
    Out: Lower + Lift + Send + Sync + 'static,
{
    StreamReader::new(access, Converted { source, convert })
}

/// A source stream with the conversion of its items.
struct Converted<S, F> {
    source: S,
    convert: F,
}

impl<D, S, F, Out> StreamProducer<D> for Converted<S, F>
where
    S: Stream + Send + Unpin + 'static,
    F: FnMut(&mut D, S::Item) -> wasmtime::Result<Out> + Send + Unpin + 'static,
    Out: Lower + Send + Sync + 'static,
{
    type Item = Out;
    type Buffer = Option<Out>;

    fn poll_produce<'a>(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, D>,
        mut destination: Destination<'a, Out, Option<Out>>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let this = self.get_mut();
        match this.source.poll_next_unpin(cx) {
            Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(item)) => {
                let converted = (this.convert)(store.data_mut(), item)?;
                destination.set_buffer(Some(converted));
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Poll::Ready(None) => Poll::Ready(Ok(StreamResult::Dropped)),
        }
    }
}

/// `source`, as the program's stream: each item's value delivered, and what comes with it held
/// until the program reads again or drops the stream, so for as long as the program handles the
/// item, as a unfreeze must last.
pub(crate) fn stream_holding<D, A, S, T, H>(
    access: Access<'_, D, A>,
    source: S,
) -> wasmtime::Result<StreamReader<T>>
where
    D: 'static,
    A: HasData + ?Sized,
    S: Stream<Item = (T, H)> + Send + Unpin + 'static,
    T: Lower + Lift + Send + Sync + 'static,
    H: Send + Unpin + 'static,
{
    stream_of_holding(access, source, |_data: &mut D, item| Ok(item))
}

/// `source`, as the program's stream, as [`stream_holding`] delivers it, each item's value
/// converted by `convert` with the store's data.
pub(crate) fn stream_of_holding<D, A, S, T, H, F, Out>(
    access: Access<'_, D, A>,
    source: S,
    convert: F,
) -> wasmtime::Result<StreamReader<Out>>
where
    D: 'static,
    A: HasData + ?Sized,
    S: Stream<Item = (T, H)> + Send + Unpin + 'static,
    H: Send + Unpin + 'static,
    F: FnMut(&mut D, T) -> wasmtime::Result<Out> + Send + Unpin + 'static,
    Out: Lower + Lift + Send + Sync + 'static,
{
    StreamReader::new(
        access,
        Holding {
            source,
            convert,
            held: None,
        },
    )
}

/// A source stream, the conversion of its items' values, and what is held for the item
/// delivered last.
struct Holding<S, F, H> {
    source: S,
    convert: F,
    held: Option<H>,
}

impl<D, S, T, H, F, Out> StreamProducer<D> for Holding<S, F, H>
where
    S: Stream<Item = (T, H)> + Send + Unpin + 'static,
    H: Send + Unpin + 'static,
    F: FnMut(&mut D, T) -> wasmtime::Result<Out> + Send + Unpin + 'static,
    Out: Lower + Send + Sync + 'static,
{
    type Item = Out;
    type Buffer = Option<Out>;

    fn poll_produce<'a>(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, D>,
        mut destination: Destination<'a, Out, Option<Out>>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let this = self.get_mut();
        // The program reads again: it has handled the item before.
        this.held = None;
        match this.source.poll_next_unpin(cx) {
            Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some((item, held))) => {
                let converted = (this.convert)(store.data_mut(), item)?;
                this.held = Some(held);
                destination.set_buffer(Some(converted));
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Poll::Ready(None) => Poll::Ready(Ok(StreamResult::Dropped)),
        }
    }
}

/// `bytes`, as the program's stream: all of them at its first read, then its end.
pub(crate) fn stream_of_bytes<D, A>(
    access: Access<'_, D, A>,
    bytes: Vec<u8>,
) -> wasmtime::Result<StreamReader<u8>>
where
    D: 'static,
    A: HasData + ?Sized,
{
    StreamReader::new(access, Bytes(Some(bytes)))
}

/// Bytes, until they are read.
struct Bytes(Option<Vec<u8>>);

impl<D> StreamProducer<D> for Bytes {
    type Item = u8;
    type Buffer = VecBuffer<u8>;

    fn poll_produce<'a>(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _store: StoreContextMut<'a, D>,
        mut destination: Destination<'a, u8, VecBuffer<u8>>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        match self.get_mut().0.take() {
            Some(bytes) => {
                destination.set_buffer(VecBuffer::from(bytes));
                Poll::Ready(Ok(StreamResult::Completed))
            }
            None => Poll::Ready(Ok(StreamResult::Dropped)),
        }
    }
}

/// How much of shared bytes one read hands the program.
const SHARED_CHUNK: usize = 1 << 20;

/// `bytes`, which the engine shares with others, as the program's stream: a chunk of them at
/// each read, as much as the program's buffer takes, copied from where they are straight into
/// it, then its end. Nothing is copied but what the program
/// reads.
pub(crate) fn stream_of_shared_bytes<D, A>(
    access: Access<'_, D, A>,
    bytes: Arc<dyn AsRef<[u8]> + Send + Sync>,
) -> wasmtime::Result<StreamReader<u8>>
where
    D: 'static,
    A: HasData + ?Sized,
{
    StreamReader::new(access, SharedBytes { bytes, read: 0 })
}

/// Shared bytes, and how many of them the program has read.
struct SharedBytes {
    bytes: Arc<dyn AsRef<[u8]> + Send + Sync>,
    read: usize,
}

impl<D> StreamProducer<D> for SharedBytes {
    type Item = u8;
    type Buffer = VecBuffer<u8>;

    fn poll_produce<'a>(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        store: StoreContextMut<'a, D>,
        destination: Destination<'a, u8, VecBuffer<u8>>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let this = self.get_mut();
        let rest = &(*this.bytes).as_ref()[this.read..];
        if rest.is_empty() {
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        let chunk = &rest[..rest.len().min(SHARED_CHUNK)];
        // Straight into the reader's buffer, the program's memory when the program reads: no
        // copy of the chunk in between.
        let mut destination = destination.as_direct(store, chunk.len());
        let count = destination.remaining().len().min(chunk.len());
        destination.remaining()[..count].copy_from_slice(&chunk[..count]);
        destination.mark_written(count);
        this.read += count;
        Poll::Ready(Ok(StreamResult::Completed))
    }
}

/// Bytes as `chunks` gives them, as the program's stream: each chunk at a read of its own, as it
/// comes, and the end with theirs.
pub(crate) fn stream_of_chunks<D, A, S>(
    access: Access<'_, D, A>,
    chunks: S,
) -> wasmtime::Result<StreamReader<u8>>
where
    D: 'static,
    A: HasData + ?Sized,
    S: Stream<Item = Vec<u8>> + Send + Unpin + 'static,
{
    StreamReader::new(access, Chunked(chunks))
}

/// Bytes in chunks, read one at a time.
struct Chunked<S>(S);

impl<D, S> StreamProducer<D> for Chunked<S>
where
    S: Stream<Item = Vec<u8>> + Send + Unpin + 'static,
{
    type Item = u8;
    type Buffer = VecBuffer<u8>;

    fn poll_produce<'a>(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        _store: StoreContextMut<'a, D>,
        mut destination: Destination<'a, u8, VecBuffer<u8>>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        match self.get_mut().0.poll_next_unpin(cx) {
            Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(chunk)) => {
                destination.set_buffer(VecBuffer::from(chunk));
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Poll::Ready(None) => Poll::Ready(Ok(StreamResult::Dropped)),
        }
    }
}

/// The bytes of a program's stream, collected until it ends, then handed on.
pub(crate) struct Collected {
    bytes: Vec<u8>,
    done: Option<oneshot::Sender<Vec<u8>>>,
}

impl Collected {
    /// A collection whose bytes go to the receiver answered with it, once the stream ends.
    pub(crate) fn new() -> (Collected, oneshot::Receiver<Vec<u8>>) {
        let (done, collected) = oneshot::channel();
        let collection = Collected {
            bytes: Vec::new(),
            done: Some(done),
        };
        (collection, collected)
    }
}

impl Drop for Collected {
    /// The stream ended: what came is handed on.
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            let _ = done.send(std::mem::take(&mut self.bytes));
        }
    }
}

impl<D> StreamConsumer<D> for Collected {
    type Item = u8;

    fn poll_consume(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        source: Source<'_, u8>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let mut source = source.as_direct(store);
        let read = source.remaining().len();
        self.get_mut().bytes.extend_from_slice(source.remaining());
        source.mark_read(read);
        Poll::Ready(Ok(StreamResult::Completed))
    }
}

/// The bytes of a program's stream, passed on as the program gives them, each write one chunk,
/// and taken only as fast as the receiver takes them; the receiver's stream ends with the
/// program's.
pub(crate) struct Chunks(mpsc::Sender<Vec<u8>>);

impl Chunks {
    /// Chunks going to `receiver`.
    pub(crate) fn new(receiver: mpsc::Sender<Vec<u8>>) -> Chunks {
        Chunks(receiver)
    }
}

impl<D> StreamConsumer<D> for Chunks {
    type Item = u8;

    /// Takes the program's write once the receiver has room for it; stops reading once the
    /// receiver is gone.
    fn poll_consume(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        source: Source<'_, u8>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let receiver = &mut self.get_mut().0;
        match receiver.poll_ready(cx) {
            Poll::Pending if finish => return Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(_gone)) => return Poll::Ready(Ok(StreamResult::Dropped)),
            Poll::Ready(Ok(())) => {}
        }
        let mut source = source.as_direct(store);
        let chunk = source.remaining().to_vec();
        source.mark_read(chunk.len());
        if !chunk.is_empty() && receiver.start_send(chunk).is_err() {
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        Poll::Ready(Ok(StreamResult::Completed))
    }
}

/// The items of a program's stream, passed on one at a time as the program writes them, each
/// taken only once the receiver has room for it; the receiver's stream ends with the program's.
pub(crate) struct Forwarded<T>(mpsc::Sender<T>);

impl<T> Forwarded<T> {
    /// Items going to `receiver`.
    pub(crate) fn new(receiver: mpsc::Sender<T>) -> Forwarded<T> {
        Forwarded(receiver)
    }
}

impl<D, T: Lift + Send + Sync + 'static> StreamConsumer<D> for Forwarded<T> {
    type Item = T;

    /// Takes the program's next item once the receiver has room for it; stops reading once the
    /// receiver is gone.
    fn poll_consume(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        mut source: Source<'_, T>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let receiver = &mut self.get_mut().0;
        match receiver.poll_ready(cx) {
            Poll::Pending if finish => return Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(_gone)) => return Poll::Ready(Ok(StreamResult::Dropped)),
            Poll::Ready(Ok(())) => {}
        }
        let mut item = None;
        source.read(store, &mut item)?;
        if let Some(item) = item
            && receiver.start_send(item).is_err()
        {
            return Poll::Ready(Ok(StreamResult::Dropped));
        }
        Poll::Ready(Ok(StreamResult::Completed))
    }
}

/// The value a future the program gives settles with, handed on; none when the program drops it
/// unwritten.
pub(crate) struct Settled<T>(Option<oneshot::Sender<T>>);

impl<T> Settled<T> {
    /// The value going to `receiver`.
    pub(crate) fn new(receiver: oneshot::Sender<T>) -> Settled<T> {
        Settled(Some(receiver))
    }
}

impl<D, T: Lift + Send + Sync + 'static> FutureConsumer<D> for Settled<T> {
    type Item = T;

    fn poll_consume(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        mut source: Source<'_, T>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<()>> {
        let mut value = None;
        source.read(store, &mut value)?;
        if let (Some(value), Some(receiver)) = (value, self.get_mut().0.take()) {
            let _ = receiver.send(value);
        }
        Poll::Ready(Ok(()))
    }
}
