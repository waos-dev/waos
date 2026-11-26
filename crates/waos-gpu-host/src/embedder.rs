//! What the embedder decides for a program's GPU, at the calls the platform has a say in: which
//! adapter a program gets, how its device is opened, when it may submit GPU work, and who is told
//! of the devices it opens. Where a view's frames come from is `surfaces`' [`SurfaceTarget`].
//!
//! The invariant: a trait here is asked from inside a call of the program's, so it answers at
//! once; a wait is a future it hands back, which the call awaits.
//!
//! [`SurfaceTarget`]: crate::SurfaceTarget

use std::future::Future;
use std::pin::Pin;

/// How an embedder chooses the adapter a program's `request-adapter` gives, as WebGPU and wgpu
/// leave a platform to: an embedder whose frames the system's compositor shows puts programs on the
/// GPU it shows from.
pub trait AdapterChooser: Send + Sync {
    /// The adapter of `instance` a program asking with `options` gets; none where there is none
    /// to give, which the program is answered as none.
    fn choose(
        &self,
        instance: &wgpu_core::global::Global,
        options: &wgpu_types::RequestAdapterOptions<wgpu_core::id::SurfaceId>,
    ) -> Option<wgpu_core::id::AdapterId>;
}

/// How an embedder opens a program's device in place of wgpu-core's `adapter_request_device`.
/// An embedder whose frames the system compositor reads needs the device made with backend
/// extensions wgpu-core does not enable, which only a device opened through the backend itself
/// and handed to `Global::create_device_from_hal` can have.
///
/// The host has checked `descriptor`'s limits against the adapter's; its features it has not, and
/// `create_device_from_hal` does not check them either, so an opener refuses a feature the adapter
/// lacks with `RequestDeviceError::UnsupportedFeature`. Whatever the opener
/// refuses reaches the program as `request-device`'s refusal.
pub trait DeviceOpener: Send + Sync {
    fn open(
        &self,
        instance: &wgpu_core::global::Global,
        adapter: wgpu_core::id::AdapterId,
        descriptor: &wgpu_types::DeviceDescriptor<wgpu_core::Label<'static>>,
    ) -> Result<
        (wgpu_core::id::DeviceId, wgpu_core::id::QueueId),
        wgpu_core::instance::RequestDeviceError,
    >;
}

/// When a program may submit GPU work. A platform that refuses GPU work from an app in the
/// background has its embedder hold a program's submissions while the program is held there;
/// wgpu-core submits GPU work at `queue.submit` alone, a queue write waiting in its pending
/// commands until the next, so that is the one call that asks. Passing the gate and submitting
/// are one step: the submission runs inside [`while_allowed`](Self::while_allowed), so a hold
/// that comes between the wait and the submission refuses it, and no submission starts once a
/// hold has taken.
pub trait SubmissionGate: Send + Sync {
    /// Resolves once the program may submit; at once when it may now.
    fn until_allowed(&self) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

    /// Runs `submit` unless the program may not submit now, keeping that from changing while
    /// it runs; answers whether it ran. A caller answered no waits on `until_allowed` and asks
    /// again. `submit` is quick and takes nothing of the embedder's.
    fn while_allowed(&self, submit: &mut dyn FnMut()) -> bool;
}

/// What an embedder is told of a program's devices: each one `request-device` opens, as the
/// program is handed it, so the embedder can wait for the GPU work on every device the program
/// holds without keeping any alive ([`Device::downgrade`]).
///
/// [`Device::downgrade`]: crate::Device::downgrade
pub trait DeviceListener: Send + Sync {
    /// The program opened `device`.
    fn opened(&self, device: &crate::Device);
}
