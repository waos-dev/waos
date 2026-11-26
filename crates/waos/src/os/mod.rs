//! What a system adds to drawing a program's frames: the memory of a hardware buffer, which the
//! system allocates so that a program's GPU device draws into it and the compositor shows it with
//! no copy between them, and a texture over that memory on the program's device. Each system has
//! its own module with the same names, and the target picks the module, so the rest of the crate
//! carries no `cfg` of its own. Apple's systems have an `IOSurface`, Android an `AHardwareBuffer`,
//! and Linux a dmabuf, each of which a program's device imports only when opened to (`vulkan`'s
//! device opener) and which is handed between that device and the compositor, and Windows a D3D12
//! texture shared by NT handle, which the shell copies into what the compositor shows; elsewhere
//! there is none yet, asking for one fails, and drawing is refused with that message. And the
//! proxy settings the system's own apps follow, which a program's HTTP follows too (`proxies`).
//!
//! The invariant: wrapping the system's APIs is what this module is for, and it is the one place
//! in the crate, with the code cache's mapping and the Windows file calls of `files`, where `unsafe`
//! is written.

#[cfg(target_os = "android")]
mod android;
#[cfg(target_vendor = "apple")]
mod apple;
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    windows
)))]
mod generic;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(target_vendor = "apple"))]
mod one_plane;
mod proxies;
#[cfg(any(target_os = "android", target_os = "linux"))]
mod vulkan;
#[cfg(windows)]
mod windows;

pub(crate) use proxies::{SystemProxies, system_proxies};

#[cfg(not(target_vendor = "apple"))]
pub(crate) use one_plane::{plane_texture, planes, read_planes};

#[cfg(target_os = "android")]
pub use android::HardwareBuffer;
#[cfg(target_os = "android")]
pub(crate) use android::*;
#[cfg(target_vendor = "apple")]
pub(crate) use apple::*;
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    windows
)))]
pub(crate) use generic::*;
#[cfg(target_os = "linux")]
pub use linux::HardwareBuffer;
#[cfg(target_os = "linux")]
pub(crate) use linux::*;
#[cfg(windows)]
pub use windows::HardwareBuffer;
#[cfg(windows)]
pub(crate) use windows::*;

/// Work run on the finisher/waiting thread after the GPU submission for a frame or hardware
/// buffer handover completes: e.g. mapping and reading back an offscreen staging buffer on Linux.
pub(crate) type FinishHandOver = Box<dyn FnOnce(&crate::gpu::wgpu_core::global::Global) + Send>;

/// What `hand_to_compositor` returns: the GPU submission to wait for, and any post-wait work
/// that finishes transferring pixels out of the GPU.
pub(crate) struct HandOver {
    pub(crate) submission: Option<crate::gpu::wgpu_core::SubmissionIndex>,
    pub(crate) finish: Option<FinishHandOver>,
}

impl HandOver {
    pub(crate) fn submission(submission: Option<crate::gpu::wgpu_core::SubmissionIndex>) -> Self {
        Self {
            submission,
            finish: None,
        }
    }
}

/// What programs' pictures are kept in: GPU memory on every system but Linux, whose compositor
/// shows the system's own buffers whatever it says it imports.
#[cfg(not(target_os = "linux"))]
pub(crate) fn buffer_kind(
    _gpu: &crate::gpu::GpuInstance,
    accepted: Option<&crate::shell::AcceptedBuffers>,
    _programs_adapter: &crate::gpu::ProgramsAdapter,
) -> crate::BufferKind {
    crate::BufferKind::GpuMemory(accepted.and_then(crate::shell::AcceptedBuffers::gpu))
}

/// The GPU `adapter` is, as the system names it to its compositor: none on a system that names
/// none, as Apple's and Android do.
#[cfg(not(any(target_os = "linux", windows)))]
pub(crate) fn adapter_gpu(
    _instance: &crate::gpu::wgpu_core::global::Global,
    _adapter: crate::gpu::wgpu_core::id::AdapterId,
) -> Option<crate::shell::GpuId> {
    None
}
