//! What a device tells the program of its loss: the engine's `lost` future, read without waiting
//! whenever the device's callbacks run (`runtime`) while a callback is set, which calls the
//! callback the program set with `Device::set_device_lost_callback`, once.
//!
//! A device dropped without being lost drops its callback uncalled, as wgpu-core does, and the
//! engine's `lost` never resolves for it.
//!
//! The invariant: the future is read only while a callback waits for it, so a loss that comes
//! before one is set reaches the one set later; the callback is replaced, not added to, as
//! wgpu's `set_device_lost_callback` replaces it, and is never called with a lock held.

use std::sync::{Mutex, MutexGuard, PoisonError};

use wasdk::FutureReader;
use wasdk::gpu::Device;
use wasdk::gpu_types as wit;
use wgpu::custom::BoxDeviceLostCallback;

use crate::convert;
use crate::runtime::read_now;

/// A device's loss and the callback it goes to.
pub(crate) struct DeviceLostCallback(Mutex<State>);

struct State {
    /// The engine's `lost`, until it resolves.
    lost: Option<FutureReader<wit::DeviceLostInfo>>,
    callback: Option<BoxDeviceLostCallback>,
}

impl std::fmt::Debug for DeviceLostCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceLostCallback").finish_non_exhaustive()
    }
}

impl DeviceLostCallback {
    /// `device`'s loss from now on, with no callback yet.
    pub(crate) fn listen(device: &Device) -> DeviceLostCallback {
        DeviceLostCallback(Mutex::new(State {
            lost: Some(device.lost()),
            callback: None,
        }))
    }

    /// Sends the loss to `callback`, dropping the one set before.
    pub(crate) fn set(&self, callback: BoxDeviceLostCallback) {
        let replaced = self.state().callback.replace(callback);
        drop(replaced);
    }

    /// Calls the callback where the device has been lost by now.
    pub(crate) fn run_if_lost(&self) {
        let lost = {
            let mut state = self.state();
            if state.callback.is_none() {
                return;
            }
            let Some(lost) = state.lost.take() else {
                return;
            };
            match read_now(lost) {
                Ok(info) => state.callback.take().map(|callback| (callback, info)),
                Err(lost) => {
                    state.lost = Some(lost);
                    None
                }
            }
        };
        if let Some((callback, info)) = lost {
            callback(
                convert::device_lost_reason_from_wit(info.reason),
                info.message,
            );
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
