//! A device's loss, as WebGPU reports it: once, with a reason and a message, to every reader of
//! the device's `lost`, whether it asked before the loss or after; and from then on no errors,
//! since none are generated from a lost device.
//!
//! wgpu-core reports a loss it detects through the closure it holds for the device. A device the
//! program destroys is lost at once, as WebGPU's `destroy()` loses it, where wgpu-core would say
//! so only at its next poll that finds the queue empty. Whichever comes first is the loss.

use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::channel::oneshot;
use futures::future::{FutureExt, Shared};
use wasmtime::component::{FutureProducer, Resource};
use wasmtime::StoreContextMut;

use crate::WasiWebGpuCtxView;

/// What a device's `lost` resolves with.
#[derive(Clone, Debug)]
pub struct DeviceLostInfo {
    pub(crate) reason: wgpu_types::DeviceLostReason,
    pub(crate) message: String,
}

/// The loss of one device: shared by every handle on the device, its error handler, and the
/// closure wgpu-core holds for it.
#[derive(Debug)]
pub struct DeviceLoss {
    happened: AtomicBool,
    /// Taken by the loss, which sends its info once.
    announcement: Mutex<Option<oneshot::Sender<DeviceLostInfo>>>,
    info: Shared<oneshot::Receiver<DeviceLostInfo>>,
}

/// The loss's info, for any number of readers, once the device is lost. It never resolves for a
/// device dropped without being lost.
pub(crate) type LostInfo = Shared<oneshot::Receiver<DeviceLostInfo>>;

impl DeviceLoss {
    pub(crate) fn new() -> Arc<DeviceLoss> {
        let (announcement, info) = oneshot::channel();
        Arc::new(DeviceLoss {
            happened: AtomicBool::new(false),
            announcement: Mutex::new(Some(announcement)),
            info: info.shared(),
        })
    }

    /// Loses the device for `reason`. A device is lost once: a later loss changes nothing.
    pub(crate) fn lose(&self, reason: wgpu_types::DeviceLostReason, message: String) {
        self.happened.store(true, Ordering::Release);
        let announcement = self.announcement.lock().unwrap().take();
        if let Some(announcement) = announcement {
            let _ = announcement.send(DeviceLostInfo { reason, message });
        }
    }

    pub(crate) fn has_happened(&self) -> bool {
        self.happened.load(Ordering::Acquire)
    }

    pub(crate) fn info(&self) -> LostInfo {
        self.info.clone()
    }

    /// The closure wgpu-core calls when it loses the device.
    pub(crate) fn reported_by_wgpu(self: &Arc<Self>) -> wgpu_core::device::DeviceLostClosure {
        let loss = Arc::clone(self);
        Box::new(move |reason, message| loss.lose(reason, message))
    }
}

/// `gpu-device.lost` as a component-model future: nothing until the device is lost, then its
/// info, handed to the program as a resource.
pub(crate) struct LostInfoProducer<T> {
    info: LostInfo,
    store: PhantomData<fn() -> T>,
}

impl<T> LostInfoProducer<T> {
    pub(crate) fn new(info: LostInfo) -> LostInfoProducer<T> {
        LostInfoProducer {
            info,
            store: PhantomData,
        }
    }
}

impl<T: WasiWebGpuCtxView + 'static> FutureProducer<T> for LostInfoProducer<T> {
    type Item = Resource<DeviceLostInfo>;

    fn poll_produce(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<T>,
        finish: bool,
    ) -> Poll<wasmtime::Result<Option<Resource<DeviceLostInfo>>>> {
        match self.get_mut().info.poll_unpin(cx) {
            Poll::Ready(Ok(info)) => {
                Poll::Ready(Ok(Some(store.data_mut().webgpu_ctx().table.push(info)?)))
            }
            // The program stopped waiting; or the device was dropped without being lost, and
            // never will be.
            Poll::Pending | Poll::Ready(Err(_)) if finish => Poll::Ready(Ok(None)),
            Poll::Pending | Poll::Ready(Err(_)) => Poll::Pending,
        }
    }
}
