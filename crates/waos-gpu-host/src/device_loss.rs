//! A device's loss: once, with a reason and a message, to every reader of the device's `lost`,
//! whether it asked before the loss or after; and from then on no errors, since none are reported
//! from a lost device.
//!
//! wgpu-core reports a loss it detects through the closure it holds for the device. A device the
//! program destroys is lost at once, where wgpu-core would say so only at its next poll that finds
//! the queue empty. Whichever comes first is the loss.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use futures::future::{FutureExt, Shared};

/// Why a device was lost, and what wgpu-core or the host said of it.
#[derive(Clone, Debug)]
pub struct DeviceLostInfo {
    pub reason: wgpu_types::DeviceLostReason,
    pub message: String,
}

/// The loss of one device: shared by every handle on the device, its errors, and the closure
/// wgpu-core holds for it.
#[derive(Debug)]
pub struct DeviceLoss {
    happened: AtomicBool,
    /// Taken by the loss, which sends its info once.
    announcement: Mutex<Option<oneshot::Sender<DeviceLostInfo>>>,
    info: LostInfo,
}

/// The loss's info, for any number of readers, once the device is lost. It never resolves for a
/// device dropped without being lost.
pub(crate) type LostInfo = Shared<oneshot::Receiver<DeviceLostInfo>>;

impl DeviceLoss {
    pub fn new() -> Arc<DeviceLoss> {
        let (announcement, info) = oneshot::channel();
        Arc::new(DeviceLoss {
            happened: AtomicBool::new(false),
            announcement: Mutex::new(Some(announcement)),
            info: info.shared(),
        })
    }

    /// Loses the device for `reason`. A device is lost once: a later loss changes nothing.
    pub fn lose(&self, reason: wgpu_types::DeviceLostReason, message: String) {
        self.happened.store(true, Ordering::Release);
        let announcement = self.announcement.lock().unwrap().take();
        if let Some(announcement) = announcement {
            let _ = announcement.send(DeviceLostInfo { reason, message });
        }
    }

    pub fn has_happened(&self) -> bool {
        self.happened.load(Ordering::Acquire)
    }

    pub fn info(&self) -> LostInfo {
        self.info.clone()
    }

    /// The closure wgpu-core calls when it loses the device.
    pub fn reported_by_wgpu(self: &Arc<Self>) -> wgpu_core::device::DeviceLostClosure {
        let loss = Arc::clone(self);
        Box::new(move |reason, message| loss.lose(reason, message))
    }
}
