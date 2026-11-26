//! What a device tells the program of its loss: the engine's `lost` future, awaited for the
//! device's life by a task on the program's executor, which calls the callback the program set
//! with `Device::set_device_lost_callback`, once.
//!
//! The task is started as the device opens, inside `request_device`'s future, as the uncaptured
//! errors' listener is (`uncaptured_errors`), and ends with the device: a device dropped without
//! being lost drops its callback uncalled, as wgpu-core does, and the engine's `lost` never
//! resolves for it.
//!
//! The invariant: one listener per device, and the callback is replaced, not added to, as wgpu's
//! `set_device_lost_callback` replaces it.

use std::future::IntoFuture;
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};

use futures::channel::oneshot;
use futures::future::{Either, select};
use wasdk::gpu::Device;
use wgpu::custom::BoxDeviceLostCallback;

use crate::convert;
use crate::runtime;

/// The callback a device's loss goes to, shared by the device, which replaces it, and the task
/// that waits for the loss.
pub(crate) struct DeviceLostCallback {
    // Replaced by the program's thread through the device, taken by the listening task.
    callback: Arc<Mutex<Option<BoxDeviceLostCallback>>>,
    /// Dropped with the device, which ends the task.
    _device: oneshot::Sender<()>,
}

impl std::fmt::Debug for DeviceLostCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceLostCallback").finish_non_exhaustive()
    }
}

impl DeviceLostCallback {
    /// Starts waiting for `device`'s loss, with no callback yet.
    pub(crate) fn listen(device: &Device) -> DeviceLostCallback {
        let callback: Arc<Mutex<Option<BoxDeviceLostCallback>>> = Arc::default();
        let (device_alive, device_gone) = oneshot::channel();
        let lost = device.lost();
        let listener = Arc::clone(&callback);
        runtime::spawn(async move {
            let lost = pin!(lost.into_future());
            let Either::Left((info, _)) = select(lost, device_gone).await else {
                return;
            };
            let callback = listener
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(callback) = callback {
                callback(
                    convert::device_lost_reason_from_wit(info.reason),
                    info.message,
                );
            }
        });
        DeviceLostCallback {
            callback,
            _device: device_alive,
        }
    }

    /// Sends the loss to `callback`, dropping the one set before.
    pub(crate) fn set(&self, callback: BoxDeviceLostCallback) {
        *self.callback.lock().unwrap_or_else(PoisonError::into_inner) = Some(callback);
    }
}
