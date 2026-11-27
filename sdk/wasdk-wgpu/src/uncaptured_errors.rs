//! Where a device's errors that no error scope caught go: the engine's stream of them
//! (`uncaptured-errors`), read without waiting whenever the device's callbacks run (`runtime`),
//! each handed to the handler the program set with `Device::on_uncaptured_error`, or else to
//! wgpu's default, which panics, as wgpu's own backend does.
//!
//! The invariant: the stream is taken once, as the device opens, and its reads never outlive a
//! call, so no task waits on it; the handler is replaced, not added to, as wgpu's
//! `on_uncaptured_error` replaces it, and is never called with a lock held.

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use wasdk::gpu::Device;
use wasdk::gpu_types as wit;
use wasdk::{StreamReader, StreamResult};

use crate::convert;

/// How many errors one read of the stream takes.
const ERRORS_PER_READ: usize = 16;

/// A device's uncaptured errors and the handler they go to.
pub(crate) struct UncapturedErrors {
    // Replaced through the device, read through the device and the queue.
    handler: Mutex<Option<Arc<dyn wgpu::UncapturedErrorHandler>>>,
    /// The engine's stream, until it ends.
    stream: Mutex<Option<StreamReader<wit::Error>>>,
}

impl std::fmt::Debug for UncapturedErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UncapturedErrors").finish_non_exhaustive()
    }
}

impl UncapturedErrors {
    /// `device`'s uncaptured errors from now on, with wgpu's default handler.
    pub(crate) fn listen(device: &Device) -> UncapturedErrors {
        UncapturedErrors {
            handler: Mutex::new(None),
            stream: Mutex::new(device.uncaptured_errors()),
        }
    }

    /// Sends the errors from now on to `handler`.
    pub(crate) fn set_handler(&self, handler: Arc<dyn wgpu::UncapturedErrorHandler>) {
        *self.handler.lock().unwrap_or_else(PoisonError::into_inner) = Some(handler);
    }

    /// Hands on, in the order reported, every error the engine has reported so far.
    pub(crate) fn handle_reported(&self) {
        for error in self.reported() {
            self.handle(convert::error_from_wit(error));
        }
    }

    /// The errors the stream holds now, read without waiting.
    fn reported(&self) -> Vec<wit::Error> {
        let mut stream = self.stream.lock().unwrap_or_else(PoisonError::into_inner);
        let mut reported = Vec::new();
        while let Some(reader) = stream.as_mut() {
            let (result, errors) = read_now(reader);
            reported.extend(errors);
            match result {
                StreamResult::Complete(0) | StreamResult::Cancelled => break,
                StreamResult::Complete(_) => {}
                StreamResult::Dropped => *stream = None,
            }
        }
        reported
    }

    fn handle(&self, error: wgpu::Error) {
        let handler = self
            .handler
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match handler {
            Some(handler) => handler(error),
            None => panic!("wgpu error: {error}\n"),
        }
    }
}

/// What one read of `reader` takes at once: the read polled once, and cancelled where it has
/// nothing yet, which can still hand back errors that came meanwhile.
fn read_now(reader: &mut StreamReader<wit::Error>) -> (StreamResult, Vec<wit::Error>) {
    let mut read = pin!(reader.read(Vec::with_capacity(ERRORS_PER_READ)));
    match read.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(read) => read,
        Poll::Pending => read.cancel(),
    }
}
