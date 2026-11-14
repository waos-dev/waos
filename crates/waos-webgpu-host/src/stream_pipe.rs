// Modified for WaOS by the waos Authors from the wasi-gfx runtime, as NOTICE says.
//! A [`Stream`] as a component-model stream, each item mapped with access to the store as it is
//! written: how `gpu-device.on-uncaptured-error` hands each error out as a resource of the
//! program's table. Folded in from the wasi-gfx runtime's `shared` crate, which also carried
//! helpers this crate never used.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Stream, StreamExt};
use wasmtime::component::{Destination, Lower, StreamProducer, StreamResult};
use wasmtime::StoreContextMut;

/// Adapts a [`Stream`] into a wasmtime [`StreamProducer`], delivering one item per read, each
/// mapped with access to the store data `D` before lowering. The closure runs inside
/// `poll_produce`, so it gets fresh store access per item (e.g. to push the item into the
/// resource table).
pub(crate) struct StreamPipeMap<S, F>(pub S, pub F);

impl<D, In, Out, S, F> StreamProducer<D> for StreamPipeMap<S, F>
where
    Out: Lower + Send + Sync + 'static,
    S: Stream<Item = In> + Send + Unpin + 'static,
    F: FnMut(&mut D, In) -> wasmtime::Result<Out> + Send + Unpin + 'static,
{
    type Item = Out;
    type Buffer = Option<Out>;

    fn poll_produce<'a>(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, D>,
        mut destination: Destination<'a, Self::Item, Self::Buffer>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        // `S: Unpin` and `F: Unpin`, so `Self: Unpin` and we can project freely.
        let this = self.get_mut();
        match this.0.poll_next_unpin(cx) {
            Poll::Pending if finish => Poll::Ready(Ok(StreamResult::Cancelled)),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(item)) => {
                let out = (this.1)(store.data_mut(), item)?;
                destination.set_buffer(Some(out));
                Poll::Ready(Ok(StreamResult::Completed))
            }
            Poll::Ready(None) => Poll::Ready(Ok(StreamResult::Dropped)),
        }
    }
}
