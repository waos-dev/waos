//! What a system without hardware buffers answers, one neither Apple's nor Android: there is
//! never one to draw into, so a canvas on this system cannot be configured, and says why; a
//! program allocates none; and it hands out no video frames, so there is never one to sample.

use std::convert::Infallible;

use anyhow::{Result, bail};

use waos_webgpu_host::Device;

use crate::BufferKind;
use crate::bindings::wasdk::platform::hardware_buffers::{
    HardwareBufferUsage, ImageError, ImageErrorKind, PixelBuffer, PixelFormat,
};
use crate::gpu::{GpuInstance, wgpu_core, wgpu_types};
use crate::hardware_buffers::Released;
use crate::os::{FinishHandOver, HandOver};
use crate::shell::{HardwareBuffer, VideoPicture};

/// The format a canvas would be configured with, were there a hardware buffer to draw into.
pub(crate) const FORMAT: wgpu_types::TextureFormat = wgpu_types::TextureFormat::Bgra8Unorm;

/// Whether a program allocates hardware buffers on this system: it does not.
pub(crate) const ALLOCATES: bool = false;

/// The largest side of a hardware buffer, were one allocated.
pub(crate) const LARGEST_SIDE: u32 = 0;

/// Whether a texture over a hardware buffer is handed to the compositor and back: there is none.
pub(crate) const HANDS_TEXTURES_OVER: bool = false;

/// One hardware buffer's memory. This system makes none, so here the type has no values:
/// [`HardwareBufferMemory::new`] and [`HardwareBufferMemory::allocate`] are the only ways to ask
/// for one, and they fail.
#[derive(Clone)]
pub(crate) struct HardwareBufferMemory(Infallible);

impl HardwareBufferMemory {
    pub(crate) fn new(
        width: u32,
        height: u32,
        usage: HardwareBufferUsage,
        mode: BufferKind,
    ) -> Result<HardwareBufferMemory> {
        let _ = (width, height, usage, mode);
        bail!(
            "drawing is not built on this system yet: the engine makes hardware buffers on Apple's systems and Android"
        )
    }

    pub(crate) fn unsupported(
        format: PixelFormat,
        usage: HardwareBufferUsage,
    ) -> Option<&'static str> {
        let _ = (format, usage);
        Some("the engine allocates hardware buffers on Apple's systems and Android only")
    }

    pub(crate) fn allocate(
        width: u32,
        height: u32,
        format: PixelFormat,
        usage: HardwareBufferUsage,
        mode: BufferKind,
    ) -> Result<HardwareBufferMemory, ImageError> {
        let _ = (width, height, format, usage, mode);
        Err(ImageError {
            kind: ImageErrorKind::Unsupported,
            message: "the engine allocates hardware buffers on Apple's systems and Android only"
                .to_owned(),
        })
    }

    pub(crate) fn of(surface: HardwareBuffer) -> HardwareBufferMemory {
        match surface {}
    }

    pub(crate) fn buffer(&self) -> HardwareBuffer {
        match self.0 {}
    }

    pub(crate) fn width(&self) -> u32 {
        match self.0 {}
    }

    pub(crate) fn height(&self) -> u32 {
        match self.0 {}
    }

    pub(crate) fn usage(&self) -> HardwareBufferUsage {
        match self.0 {}
    }

    pub(crate) fn bytes(&self) -> u64 {
        match self.0 {}
    }

    pub(crate) fn read_pixels(&self) -> Result<PixelBuffer, ImageError> {
        match self.0 {}
    }

    pub(crate) fn texture_over(
        &self,
        _gpu: &wgpu_core::global::Global,
        _device: wgpu_core::id::DeviceId,
        _usage: wgpu_types::TextureUsages,
        _view_formats: &[wgpu_types::TextureFormat],
    ) -> Result<wgpu_core::id::TextureId> {
        match self.0 {}
    }

    pub(crate) fn sampled_texture(
        &self,
        _gpu: &wgpu_core::global::Global,
        _device: wgpu_core::id::DeviceId,
        _queue: wgpu_core::id::QueueId,
    ) -> Result<wgpu_core::id::TextureId> {
        match self.0 {}
    }

    pub(crate) fn encode_readback(
        &self,
        _gpu: &GpuInstance,
        _device: wgpu_core::id::DeviceId,
        _encoder: wgpu_core::id::CommandEncoderId,
        _texture: wgpu_core::id::TextureId,
    ) -> Result<Option<FinishHandOver>> {
        match self.0 {}
    }

    pub(crate) fn in_use(&self) -> InUse {
        match self.0 {}
    }

    pub(crate) fn after_reads(&self, _released: Released) -> Released {
        match self.0 {}
    }

    pub(crate) fn program_texture(
        &self,
        _gpu: &GpuInstance,
        _device: &waos_webgpu_host::Device,
        _usage: wgpu_types::TextureUsages,
        _release: Box<dyn FnOnce() + Send + Sync>,
    ) -> Result<(
        std::sync::Arc<waos_webgpu_host::GpuObject<wgpu_core::id::TextureId>>,
        waos_webgpu_host::TextureAttributes,
    )> {
        match self.0 {}
    }
}

/// Hands a frame to the compositor's side: there is never a frame here, so only what the program
/// wrote is submitted.
pub(crate) fn hand_to_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) -> HandOver {
    let _ = (gpu, texture, usage);
    HandOver::submission(device.submit_pending_writes())
}

/// Takes a frame back from the compositor: there is never a frame here.
pub(crate) fn take_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    let _ = (gpu, device, texture, usage);
}

/// Takes a program's texture back from the compositor: there is never one here.
pub(crate) fn take_back_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    let _ = (gpu, device, texture, usage);
}

/// A hardware buffer marked in use. This system makes none, so here the type has no values.
pub(crate) struct InUse(Infallible);

/// A video's frame as the engine holds it. This system hands out none, so here the type has no
/// values.
#[derive(Clone)]
pub(crate) struct FrameMemory(Infallible);

impl FrameMemory {
    pub(crate) fn new(frame: VideoPicture) -> FrameMemory {
        match frame {}
    }

    pub(crate) fn hardware_buffer(&self) -> Result<HardwareBufferMemory> {
        match self.0 {}
    }
}
