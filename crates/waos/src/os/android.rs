//! An `AHardwareBuffer` a program's WebGPU device draws into and the compositor shows, with the
//! same bytes under both, and the texture over it on the program's wgpu-core device: Vulkan imports
//! the buffer's memory into an image (`imported_image`), on a device opened to import it
//! (`device_opener`), and wgpu-core takes the image as a texture of its own. Unlike Apple's
//! `IOSurface`, the compositor reads the buffer outside Vulkan, so the image is handed to it: a
//! frame's image is released to the compositor's queue family once the work that drew it is
//! submitted, and acquired back before it is drawn into again (`queue_ownership`). And a program's
//! own hardware buffers, `AHardwareBuffer`s it allocates with the usage it names, drawn or sampled
//! through a texture over the buffer handed to the program as its `gpu-texture`, and read while the
//! buffer is locked for the CPU. A screenshot samples a view's pictures through textures over their
//! buffers on a device of its own, holding back a picture's release to its program while it reads
//! it, since Android keeps no use count the compositor and the engine share, as `IOSurface`'s.
//!
//! An `AHardwareBuffer` is what crosses between the program's device and the shell's: the engine
//! makes a texture on the program's device over it, and the shell sets it on a surface control.

// Wrapping the system's allocation into a texture, and handing its image between queue families,
// is what this module is for; the rest of the crate has no `unsafe` but where `code_cache` maps
// compiled code.
#![allow(unsafe_code)]

mod imported_image;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use anyhow::{Result, bail};
use ndk::hardware_buffer::{
    HardwareBufferDesc, HardwareBufferRef, HardwareBufferUsage as AndroidUsage,
};
use ndk::hardware_buffer_format::HardwareBufferFormat;
use waos_webgpu_host::wasi::webgpu::webgpu::{
    GpuTextureDimension, GpuTextureFormat, GpuTextureUsage,
};
use waos_webgpu_host::{Device, GpuObject, TextureAttributes};

use crate::HardwareBufferMode;
use crate::bindings::wasdk::platform::hardware_buffers::{
    AlphaType, HardwareBufferUsage, ImageError, ImageErrorKind, PixelBuffer, PixelFormat,
};
use crate::gpu::{GpuInstance, wgpu_core, wgpu_types};
use crate::hardware_buffers::Released;
use crate::os::{FinishHandOver, HandOver};
use crate::shell::VideoPicture;
use crate::sync::locked;

pub(crate) use super::vulkan::device_opener::HardwareBufferImports;
use super::vulkan::queue_ownership::{Transfer, handed_state, submit_transfer};
use imported_image::ImportedTexture;

/// The format a hardware buffer is allocated in, and so the only one a canvas can be configured
/// with: WebGPU's preferred canvas format on this platform. Android's public formats have no BGRA.
pub(crate) const FORMAT: wgpu_types::TextureFormat = wgpu_types::TextureFormat::Rgba8Unorm;

/// What a device must enable to import an `AHardwareBuffer` and hand it to the compositor.
pub(crate) const IMPORT_EXTENSIONS: &[&std::ffi::CStr] = &[
    ash::android::external_memory_android_hardware_buffer::NAME,
    ash::ext::queue_family_foreign::NAME,
];

/// Bytes one pixel of that format occupies.
const BYTES_PER_PIXEL: u32 = 4;

/// Whether a program allocates hardware buffers on this system.
pub(crate) const ALLOCATES: bool = true;

/// The largest side of a hardware buffer a program allocates, in pixels: Vulkan's guaranteed
/// `maxImageDimension2D`.
pub(crate) const LARGEST_SIDE: u32 = 4096;

/// Whether a texture over a hardware buffer is handed to the compositor and back: on Android its
/// image is released to the compositor's queue family and acquired back (`queue_ownership`).
pub(crate) const HANDS_TEXTURES_OVER: bool = true;

/// GPU memory the system's compositor can show, as the shell hands it and is handed it: the
/// `AHardwareBuffer`, a reference on it, which Waft's Android host shows. Cloning it is an
/// `AHardwareBuffer_acquire`.
#[derive(Clone, Debug)]
pub struct HardwareBuffer(HardwareBufferRef);

// SAFETY: an `AHardwareBuffer` is made to be shared between threads and processes: its reference
// count is atomic, and its memory is written only by the GPU work and the locks the engine orders.
// `ndk` cannot say so of its pointer; Waft's Android host vouches for the same.
unsafe impl Send for HardwareBuffer {}
unsafe impl Sync for HardwareBuffer {}

impl HardwareBuffer {
    /// The `AHardwareBuffer` the buffer is.
    pub fn buffer(&self) -> &HardwareBufferRef {
        &self.0
    }
}

impl From<HardwareBufferRef> for HardwareBuffer {
    fn from(buffer: HardwareBufferRef) -> HardwareBuffer {
        HardwareBuffer(buffer)
    }
}

/// One hardware buffer's memory, as the engine holds it: the `AHardwareBuffer`, which a program's
/// device draws into through a texture made over it for each frame, and which the shell is handed
/// as the [`HardwareBuffer`] it shows. Cloning it is a reference more: whoever still holds one
/// keeps the pixels alive.
#[derive(Clone)]
pub(crate) struct HardwareBufferMemory {
    buffer: HardwareBuffer,
    width: u32,
    height: u32,
}

impl HardwareBufferMemory {
    /// Allocates an RGBA8 buffer of `width` by `height` physical pixels, for `usage`.
    pub(crate) fn new(
        width: u32,
        height: u32,
        usage: HardwareBufferUsage,
        _mode: HardwareBufferMode,
    ) -> Result<HardwareBufferMemory> {
        let description = HardwareBufferDesc {
            width,
            height,
            layers: 1,
            format: HardwareBufferFormat::R8G8B8A8_UNORM,
            usage: android_usage(usage),
            stride: 0,
        };
        let buffer = ndk::hardware_buffer::HardwareBuffer::allocate(description)
            .map_err(|error| anyhow::anyhow!("allocating an AHardwareBuffer: {error}"))?;
        Ok(HardwareBufferMemory {
            buffer: HardwareBuffer(buffer),
            width,
            height,
        })
    }

    /// A program's buffer of `width` by `height` pixels in `format`, for `usage`, sides the
    /// program checked: `unsupported` for BGRA, which Android's buffers are not allocated in, and
    /// `out-of-memory` where the system allocates none.
    pub(crate) fn allocate(
        width: u32,
        height: u32,
        format: PixelFormat,
        usage: HardwareBufferUsage,
        mode: HardwareBufferMode,
    ) -> Result<HardwareBufferMemory, ImageError> {
        if format != PixelFormat::Rgba8 {
            return Err(ImageError {
                kind: ImageErrorKind::Unsupported,
                message: "Android allocates hardware buffers in RGBA".to_owned(),
            });
        }
        HardwareBufferMemory::new(width, height, usage, mode).map_err(|error| ImageError {
            kind: ImageErrorKind::OutOfMemory,
            message: format!("{error:#}"),
        })
    }

    /// The memory of `buffer`, a hardware buffer the shell handed over: one a layer shows.
    pub(crate) fn of(buffer: HardwareBuffer) -> HardwareBufferMemory {
        let description = buffer.0.describe();
        HardwareBufferMemory {
            buffer,
            width: description.width,
            height: description.height,
        }
    }

    /// The hardware buffer as the shell shows it. A reference, not a copy.
    pub(crate) fn buffer(&self) -> HardwareBuffer {
        self.buffer.clone()
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// What the buffer can be used for: what the `AHardwareBuffer` says it was allocated for,
    /// whoever allocated it.
    pub(crate) fn usage(&self) -> HardwareBufferUsage {
        usage_of(self.buffer.0.describe().usage)
    }

    /// The bytes the buffer's rows take: what a program is counted for holding it.
    pub(crate) fn bytes(&self) -> u64 {
        let description = self.buffer.0.describe();
        u64::from(description.stride) * u64::from(BYTES_PER_PIXEL) * u64::from(description.height)
    }

    /// The buffer's pixels copied into the program's memory, rows as they lie, RGBA and
    /// premultiplied, while it is locked for the CPU to read: `unsupported` for a buffer in another
    /// format. A buffer whose usage lacks `cpu-read` is the caller's to refuse
    /// (`hardware_buffers::program_buffer`).
    pub(crate) fn read_pixels(&self) -> Result<PixelBuffer, ImageError> {
        let description = self.buffer.0.describe();
        if description.format != HardwareBufferFormat::R8G8B8A8_UNORM {
            return Err(ImageError {
                kind: ImageErrorKind::Unsupported,
                message: format!("a buffer in {:?}, not RGBA8", description.format),
            });
        }
        let row_bytes = description.stride * BYTES_PER_PIXEL;
        let length = row_bytes as usize * description.height as usize;
        let address = self
            .buffer
            .0
            .lock(AndroidUsage::CPU_READ_RARELY, None, None)
            .map_err(|error| ImageError {
                kind: ImageErrorKind::Other,
                message: format!("the system did not lock the buffer's pixels: {error}"),
            })?;
        // SAFETY: the lock maps the whole buffer for reading, `stride` pixels of 4 bytes a row,
        // `height` rows; the slice is copied before the unlock below.
        let bytes = unsafe { std::slice::from_raw_parts(address.cast::<u8>(), length) }.to_vec();
        let _ = self.buffer.0.unlock();
        Ok(PixelBuffer {
            width: self.width,
            height: self.height,
            row_bytes,
            format: PixelFormat::Rgba8,
            alpha: AlphaType::Premultiplied,
            bytes,
        })
    }

    /// A texture over the buffer's memory on the program's `device` of `gpu`, for `usage`, as the
    /// program's `gpu-texture` once connected to the device, with what it answers of itself, its
    /// pixels taken as written; `release` is called once wgpu has freed it, once the GPU is done
    /// with it. A failure the system answers is the engine's.
    ///
    /// On a lost device the texture is invalid, as one made there is.
    pub(crate) fn program_texture(
        &self,
        gpu: &GpuInstance,
        device: &Device,
        usage: wgpu_types::TextureUsages,
        release: wgpu_hal::DropCallback,
    ) -> Result<(Arc<GpuObject<wgpu_core::id::TextureId>>, TextureAttributes)> {
        let imported = ImportedTexture {
            label: "waos program hardware buffer",
            usage,
            view_formats: &[],
            arrives: arriving_state(usage),
        };
        let size = [self.width, self.height];
        let texture = imported.over(&self.buffer, size, gpu, device.device_id(), Some(release))?;
        let attributes = TextureAttributes {
            size: wgpu_types::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: GpuTextureDimension::D2,
            format: GpuTextureFormat::Rgba8unorm,
            usage: gpu_usage(usage),
            texture_binding_view_dimension: None,
        };
        Ok((Arc::new(GpuObject::new(texture, gpu)), attributes))
    }

    /// A texture over the buffer's memory on `device` of `gpu`, which the GPU samples, its pixels
    /// taken as written: a picture a screenshot reads. A failure the system answers is the
    /// engine's.
    pub(crate) fn sampled_texture(
        &self,
        gpu: &wgpu_core::global::Global,
        device: wgpu_core::id::DeviceId,
        _queue: wgpu_core::id::QueueId,
    ) -> Result<wgpu_core::id::TextureId> {
        let sampled = ImportedTexture {
            label: "waos screenshot picture",
            usage: wgpu_types::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
            arrives: wgpu_types::TextureUses::RESOURCE,
        };
        sampled.over(&self.buffer, [self.width, self.height], gpu, device, None)
    }

    /// The buffer marked as read by a screenshot until what is answered goes: a release the engine
    /// handed the shell with it (`after_reads`) waits for it meanwhile.
    pub(crate) fn in_use(&self) -> InUse {
        locked(&READS).entry(self.key()).or_default().readers += 1;
        InUse(self.clone())
    }

    /// `released`, a release the engine hands the shell with the buffer, run once no screenshot
    /// reads the buffer: at once where none does, or as the last of them is done. Android's
    /// release comes from the compositor's fence, which knows nothing of the engine's readers.
    pub(crate) fn after_reads(&self, released: Released) -> Released {
        let key = self.key();
        Box::new(move || {
            let mut reads = locked(&READS);
            match reads.get_mut(&key) {
                Some(read) => read.waiting.push(released),
                None => {
                    drop(reads);
                    released();
                }
            }
        })
    }

    /// A wgpu texture over the hardware buffer, on the program's `device` of `gpu`, for `usage` and
    /// viewable in `view_formats`: what the program configured its canvas for. One per frame: the
    /// program lets go of it once the frame is presented, which destroys the wgpu texture and the
    /// image under it, while the buffer underneath outlives every one of them.
    ///
    /// On a lost device wgpu-core answers an invalid texture, with the loss as its error: the
    /// texture is handed out as it is, and the program hears of the loss through `lost`.
    pub(crate) fn texture_over(
        &self,
        gpu: &wgpu_core::global::Global,
        device: wgpu_core::id::DeviceId,
        usage: wgpu_types::TextureUsages,
        view_formats: &[wgpu_types::TextureFormat],
    ) -> Result<wgpu_core::id::TextureId> {
        let drawn = ImportedTexture {
            label: "waos hardware buffer",
            usage,
            view_formats,
            // A new image over the buffer, holding the last frame's pixels, which nothing reads:
            // the program draws the whole canvas.
            arrives: wgpu_types::TextureUses::UNINITIALIZED,
        };
        drawn.over(&self.buffer, [self.width, self.height], gpu, device, None)
    }

    pub(crate) fn encode_readback(
        &self,
        _gpu: &GpuInstance,
        _device: wgpu_core::id::DeviceId,
        _encoder: wgpu_core::id::CommandEncoderId,
        _texture: wgpu_core::id::TextureId,
    ) -> Result<Option<FinishHandOver>> {
        Ok(None)
    }

    /// What tells this buffer's screenshots apart from another's: its `AHardwareBuffer`, unique
    /// while anyone holds a reference on it, as a reader and a release waiting for one do.
    fn key(&self) -> usize {
        self.buffer.0.as_ptr() as usize
    }
}

/// Hands a frame the program drew into `texture`, over a hardware buffer, to the compositor once
/// its work is done: what the program wrote on `device`'s queue and no submission carried is
/// submitted, with the release of the texture's image to the compositor's queue family, in the
/// layout its state of `usage` gives. Answers that submission, after every one that drew the
/// frame, for the wait before the hand-over; none where wgpu-core refused it, as on a lost
/// device, said so. GPU work, submitted only where the program's freeze allows it.
pub(crate) fn hand_to_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) -> HandOver {
    let submission = submit_transfer(gpu, device, texture, handed_state(usage), Transfer::Release)
        .inspect_err(|error| {
            if !device.is_lost() {
                tracing::warn!("a frame is handed to the compositor unreleased: {error:#}");
            }
        })
        .ok();
    HandOver::submission(submission)
}

/// Takes `texture`, a frame's over a hardware buffer, back from the compositor before the program
/// draws into it: its image is acquired from the compositor's queue family, and moved to the
/// state of `usage` a frame is handed over in. A failure, as on a lost device, is said, and the
/// frame drawn without it. GPU work, submitted only where the program's freeze allows it.
pub(crate) fn take_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    let taken = submit_transfer(gpu, device, texture, handed_state(usage), Transfer::Acquire);
    if let Err(error) = taken
        && !device.is_lost()
    {
        tracing::warn!("a frame is drawn without taking it back from the compositor: {error:#}");
    }
}

/// Takes `texture`, a program's over its own hardware buffer, which `hand_to_compositor` released,
/// back for the program's device once the compositor, the CPU or the codecs are done with the
/// buffer: its image is acquired from their queue family in the layout it was released in, its
/// pixels kept. A failure, as on a lost device, is said. GPU work, submitted only where the
/// program's freeze allows it.
pub(crate) fn take_back_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    let taken = submit_transfer(
        gpu,
        device,
        texture,
        handed_state(usage),
        Transfer::AcquireBack,
    );
    if let Err(error) = taken
        && !device.is_lost()
    {
        tracing::warn!("a hardware buffer is not taken back from the compositor: {error:#}");
    }
}

/// The screenshots reading each hardware buffer now, by its key, and the releases waiting for
/// them: the use count Android does not keep.
static READS: LazyLock<Mutex<HashMap<usize, Reads>>> = LazyLock::new(Mutex::default);

/// One buffer's readers, and the releases that wait for the last of them.
#[derive(Default)]
struct Reads {
    readers: usize,
    waiting: Vec<Released>,
}

/// A hardware buffer read by a screenshot, until this goes. Android has no use count the
/// compositor and the engine share, as `IOSurface`'s, so the engine keeps its own.
pub(crate) struct InUse(HardwareBufferMemory);

impl Drop for InUse {
    fn drop(&mut self) {
        let key = self.0.key();
        let waiting = {
            let mut reads = locked(&READS);
            let Some(read) = reads.get_mut(&key) else {
                return;
            };
            read.readers -= 1;
            if read.readers > 0 {
                return;
            }
            reads
                .remove(&key)
                .map(|read| read.waiting)
                .unwrap_or_default()
        };
        // The releases run here, outside the lock, since each reaches the program's pool.
        for released in waiting {
            released();
        }
    }
}

/// A video's frame as the engine holds it: its RGBA8 `AHardwareBuffer`, the frame's own memory,
/// which no pool hands out again: cloning it is a reference more, and the buffer is freed once
/// nobody holds one.
#[derive(Clone)]
pub(crate) struct FrameMemory {
    frame: VideoPicture,
}

impl FrameMemory {
    pub(crate) fn new(frame: VideoPicture) -> FrameMemory {
        FrameMemory { frame }
    }

    /// The frame's memory, in RGBA8, as the shell hands frames out. A failure is the shell's,
    /// which handed out a frame of another kind.
    pub(crate) fn hardware_buffer(&self) -> Result<HardwareBufferMemory> {
        let format = self.frame.0.describe().format;
        if format != HardwareBufferFormat::R8G8B8A8_UNORM {
            bail!("a video's frame is RGBA8, as the shell hands it out, not {format:?}")
        }
        Ok(HardwareBufferMemory::of(self.frame.clone()))
    }
}

/// Android's usage for a buffer allocated for `usage`.
fn android_usage(usage: HardwareBufferUsage) -> AndroidUsage {
    [
        (
            HardwareBufferUsage::GPU_SAMPLED,
            AndroidUsage::GPU_SAMPLED_IMAGE,
        ),
        (
            HardwareBufferUsage::GPU_COLOR_OUTPUT,
            AndroidUsage::GPU_COLOR_OUTPUT,
        ),
        (
            HardwareBufferUsage::COMPOSER_OVERLAY,
            AndroidUsage::COMPOSER_OVERLAY,
        ),
        (HardwareBufferUsage::CPU_READ, AndroidUsage::CPU_READ_RARELY),
    ]
    .into_iter()
    .filter(|(asked, _)| usage.contains(*asked))
    .fold(AndroidUsage::empty(), |all, (_, flag)| all | flag)
}

/// The usage of a buffer Android allocated for `usage`, each CPU read, rare or often, as
/// `cpu-read`; the uses the WIT names no flag for are left out.
fn usage_of(usage: AndroidUsage) -> HardwareBufferUsage {
    [
        (
            AndroidUsage::GPU_SAMPLED_IMAGE,
            HardwareBufferUsage::GPU_SAMPLED,
        ),
        (
            AndroidUsage::GPU_COLOR_OUTPUT,
            HardwareBufferUsage::GPU_COLOR_OUTPUT,
        ),
        (
            AndroidUsage::COMPOSER_OVERLAY,
            HardwareBufferUsage::COMPOSER_OVERLAY,
        ),
        (AndroidUsage::CPU_READ_RARELY, HardwareBufferUsage::CPU_READ),
        (AndroidUsage::CPU_READ_OFTEN, HardwareBufferUsage::CPU_READ),
    ]
    .into_iter()
    .filter(|(android, _)| usage.contains(*android))
    .fold(HardwareBufferUsage::empty(), |all, (_, flag)| all | flag)
}

/// The state a program's texture of `usage` arrives in, which its usage allows; its pixels are
/// taken as written, whoever wrote them: the program's earlier work.
fn arriving_state(usage: wgpu_types::TextureUsages) -> wgpu_types::TextureUses {
    if usage.contains(wgpu_types::TextureUsages::TEXTURE_BINDING) {
        wgpu_types::TextureUses::RESOURCE
    } else if usage.contains(wgpu_types::TextureUsages::RENDER_ATTACHMENT) {
        wgpu_types::TextureUses::COLOR_TARGET
    } else {
        wgpu_types::TextureUses::COPY_SRC
    }
}

/// `usage` as the WebGPU host tells the program its texture's usage.
fn gpu_usage(usage: wgpu_types::TextureUsages) -> GpuTextureUsage {
    [
        (
            wgpu_types::TextureUsages::COPY_SRC,
            GpuTextureUsage::COPY_SRC,
        ),
        (
            wgpu_types::TextureUsages::COPY_DST,
            GpuTextureUsage::COPY_DST,
        ),
        (
            wgpu_types::TextureUsages::TEXTURE_BINDING,
            GpuTextureUsage::TEXTURE_BINDING,
        ),
        (
            wgpu_types::TextureUsages::RENDER_ATTACHMENT,
            GpuTextureUsage::RENDER_ATTACHMENT,
        ),
    ]
    .into_iter()
    .filter(|(wgpu, _)| usage.contains(*wgpu))
    .fold(GpuTextureUsage::empty(), |all, (_, webgpu)| all | webgpu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_usage_allocated_is_answered_back_and_every_cpu_read_is_cpu_read() {
        let all = HardwareBufferUsage::all();
        assert_eq!(usage_of(android_usage(all)), all);
        assert_eq!(
            usage_of(AndroidUsage::CPU_READ_OFTEN | AndroidUsage::CPU_WRITE_OFTEN),
            HardwareBufferUsage::CPU_READ
        );
        assert_eq!(
            usage_of(AndroidUsage::empty()),
            HardwareBufferUsage::empty()
        );
    }
}
