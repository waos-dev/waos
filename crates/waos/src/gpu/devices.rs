//! The devices a program opened, as the engine keeps them to wait for the GPU work on each once
//! the program stands still: every one, a compute-only program's included, since the engine is
//! told of each at `request-device`; weakly, so a device the program let go of is gone with it,
//! and its work with it.
//!
//! The invariant: the wait blocks, so it runs on a thread of its own and never on the thread
//! that awaits the program's stop, and it counts as the program's GPU work in flight until it
//! is over.

use std::sync::{Arc, Mutex};
use std::thread;

use waos_gpu_host::{Device, DeviceListener, WeakDevice};

use super::in_flight::InFlight;
use super::{GpuInstance, wgpu_types};
use crate::sync::locked;

/// The devices one program opened and still holds.
pub(crate) struct OpenedDevices {
    gpu: GpuInstance,
    devices: Mutex<Vec<WeakDevice>>,
}

impl OpenedDevices {
    /// None opened yet, on `gpu`.
    pub(crate) fn new(gpu: GpuInstance) -> Arc<OpenedDevices> {
        Arc::new(OpenedDevices {
            gpu,
            devices: Mutex::default(),
        })
    }

    /// Waits for everything submitted on every device the program still holds, on a thread of
    /// its own, counted in `in_flight` until the wait is over. Nothing when it holds none.
    pub(crate) fn wait_for_all(&self, in_flight: &Arc<InFlight>) {
        let held: Vec<_> = locked(&self.devices)
            .iter()
            .filter_map(WeakDevice::upgrade)
            .collect();
        if held.is_empty() {
            return;
        }
        let finishing = in_flight.begin();
        let gpu = Arc::clone(&self.gpu);
        thread::Builder::new()
            .name("gpu work waited for".into())
            .spawn(move || {
                for device in held {
                    let waited = gpu.device_poll(
                        device.id(),
                        wgpu_types::PollType::Wait {
                            submission_index: None,
                            timeout: None,
                        },
                    );
                    if let Err(error) = waited {
                        tracing::warn!("waiting for a program's GPU work: {error:#?}");
                    }
                }
                drop(finishing);
            })
            .expect("a thread to wait for a program's GPU work on");
    }
}

impl DeviceListener for OpenedDevices {
    fn opened(&self, device: &Device) {
        let mut devices = locked(&self.devices);
        devices.retain(|device| device.upgrade().is_some());
        devices.push(device.downgrade());
    }
}
