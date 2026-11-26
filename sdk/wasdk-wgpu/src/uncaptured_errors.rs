//! Where a device's errors that no error scope caught go: the engine's stream of them
//! (`uncaptured-errors`), read for the device's life by a task on the program's executor, each
//! handed to the handler the program set with `Device::on_uncaptured_error`, or else to wgpu's
//! default, which panics, as wgpu's own backend does.
//!
//! The task is started as the device opens, inside `request_device`'s future, which yields once
//! after it, so the task is the executor's that awaited the future and not a later
//! `wasdk::block_on`'s, which would wait for it to end. A device opened inside a `block_on` keeps
//! that wait from returning.
//!
//! The invariant: the stream is taken once, by the one listener, and the handler is replaced, not
//! added to, as wgpu's `on_uncaptured_error` replaces it.

use std::sync::{Arc, Mutex, PoisonError};

use futures::StreamExt;
use wasdk::gpu::Device;

use crate::convert;
use crate::runtime;

/// The handler a device's uncaptured errors go to, shared by the device, which replaces it, and
/// the task that reads the errors.
#[derive(Clone)]
pub(crate) struct UncapturedErrors {
    // Replaced by the program's thread through the device, read by the listening task.
    handler: Arc<Mutex<Option<Arc<dyn wgpu::UncapturedErrorHandler>>>>,
}

impl std::fmt::Debug for UncapturedErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UncapturedErrors").finish_non_exhaustive()
    }
}

impl UncapturedErrors {
    /// Starts listening to `device`'s uncaptured errors, with wgpu's default handler.
    pub(crate) fn listen(device: &Device) -> UncapturedErrors {
        let errors = UncapturedErrors {
            handler: Arc::default(),
        };
        if let Some(stream) = device.uncaptured_errors() {
            let listener = errors.clone();
            let mut stream = stream.into_stream();
            runtime::spawn(async move {
                while let Some(error) = stream.next().await {
                    listener.handle(convert::error_from_wit(error));
                }
            });
        }
        errors
    }

    /// Sends the errors from now on to `handler`.
    pub(crate) fn set_handler(&self, handler: Arc<dyn wgpu::UncapturedErrorHandler>) {
        *self.handler.lock().unwrap_or_else(PoisonError::into_inner) = Some(handler);
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
