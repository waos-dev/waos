//! A dmabuf a program's WebGPU device draws into and the compositor shows, with the same bytes
//! under both, and the texture over it on the program's wgpu-core device: GBM allocates it on the
//! first render node, linear and exported read-write (`dmabuf`), Vulkan imports its memory into an
//! image in its modifier and plane layout (`imported_image`), on a device opened to import it
//! (`vulkan::device_opener`), and wgpu-core takes the image as a texture of its own. As on Android,
//! the compositor reads the buffer outside Vulkan, so a frame's image is released to the foreign
//! queue family once the work that drew it is submitted and acquired back before it is drawn into
//! again (`vulkan::queue_ownership`); and before that acquire, the program's thread waits for the
//! fence the compositor's reads left on the dmabuf, which a compositor may still hold when it gives
//! the buffer back (measured: a few of 187 buffers came back with a fence still pending).
//! Linear memory serves every use, so a buffer answers every usage, as Apple's do; the CPU reads it
//! between `DMA_BUF_IOCTL_SYNC`'s start and end.
//!
//! A dmabuf is what crosses between the program's device and the shell's: the engine makes a
//! texture on the program's device over it, and the shell hands Waft a duplicate of its fd, which
//! Waft makes a `wl_buffer` of for a subsurface.

// Wrapping the system's allocation into a texture, and handing its image between queue families,
// is what this module is for; the rest of the crate has no `unsafe` but where `code_cache` maps
// compiled code.
#![allow(unsafe_code)]

mod dmabuf;
mod imported_image;

use std::collections::HashMap;
use std::convert::Infallible;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::{Arc, LazyLock, Mutex};

use anyhow::Result;
use waos_webgpu_host::wasi::webgpu::webgpu::{
    GpuTextureDimension, GpuTextureFormat, GpuTextureUsage,
};
use waos_webgpu_host::{Device, GpuObject, TextureAttributes};

use crate::bindings::wasdk::platform::hardware_buffers::{
    AlphaType, HardwareBufferUsage, ImageError, ImageErrorKind, PixelBuffer, PixelFormat,
};
use crate::gpu::{GpuInstance, wgpu_core, wgpu_types};
use crate::hardware_buffers::Released;
use crate::shell::VideoPicture;
use crate::sync::locked;

pub(crate) use super::vulkan::device_opener::HardwareBufferImports;
use super::vulkan::queue_ownership::{Transfer, handed_state, submit_transfer};
use dmabuf::Dmabuf;
use imported_image::ImportedTexture;

/// The format a hardware buffer is allocated in, and so the only one a canvas can be configured
/// with: WebGPU's preferred canvas format on this platform. Android's public formats have no BGRA.
pub(crate) const FORMAT: wgpu_types::TextureFormat = wgpu_types::TextureFormat::Bgra8Unorm;

/// What a device must enable to import a dmabuf and hand it to the compositor.
pub(crate) const IMPORT_EXTENSIONS: &[&std::ffi::CStr] = &[
    ash::khr::external_memory_fd::NAME,
    ash::ext::external_memory_dma_buf::NAME,
    ash::ext::image_drm_format_modifier::NAME,
    ash::ext::queue_family_foreign::NAME,
];

/// Whether a program allocates hardware buffers on this system.
pub(crate) const ALLOCATES: bool = true;

/// The largest side of a hardware buffer a program allocates, in pixels: Vulkan's guaranteed
/// `maxImageDimension2D`.
pub(crate) const LARGEST_SIDE: u32 = 4096;

/// Whether a texture over a hardware buffer is handed to the compositor and back: on Linux its
/// image is released to the foreign queue family and acquired back (`vulkan::queue_ownership`).
pub(crate) const HANDS_TEXTURES_OVER: bool = true;

/// GPU memory the system's compositor can show, as the shell hands it and is handed it: a
/// dmabuf, which Waft's Linux host shows. Cloning it is a hold more on the same dmabuf.
#[derive(Clone)]
pub struct HardwareBuffer(Arc<ShellsDmabuf>);

/// A dmabuf, and what the shell made of it once, which lives as long as the dmabuf: Waft's buffer
/// over it, which keeps the `wl_buffer` the compositor made of it, so a pool's buffer presented
/// every frame is made a `wl_buffer` once.
struct ShellsDmabuf {
    dmabuf: Dmabuf,
    shells: std::sync::OnceLock<Arc<dyn std::any::Any + Send + Sync>>,
}

impl HardwareBuffer {
    /// A dmabuf another host made: `fd`, `size` ARGB8888 pixels, `stride` bytes a row from
    /// `offset`, in the layout `modifier` names.
    pub fn new(
        fd: OwnedFd,
        size: [u32; 2],
        stride: u32,
        offset: u32,
        modifier: u64,
    ) -> HardwareBuffer {
        HardwareBuffer::over(Dmabuf::of(fd, size, stride, offset, modifier))
    }

    fn over(dmabuf: Dmabuf) -> HardwareBuffer {
        HardwareBuffer(Arc::new(ShellsDmabuf {
            dmabuf,
            shells: std::sync::OnceLock::new(),
        }))
    }

    /// What the shell makes of the buffer, `make`'s answer, made at the first call and kept as
    /// long as the buffer is; `None` where an earlier call made something of another type.
    pub fn shells<T: std::any::Any + Send + Sync>(
        &self,
        make: impl FnOnce() -> T,
    ) -> Option<Arc<T>> {
        let made = self.0.shells.get_or_init(|| Arc::new(make()));
        Arc::clone(made).downcast::<T>().ok()
    }

    /// The dmabuf's fd, which a reader duplicates rather than takes.
    pub fn fd(&self) -> &OwnedFd {
        &self.0.dmabuf.fd
    }

    /// Its width and height in pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.0.dmabuf.width, self.0.dmabuf.height]
    }

    /// The bytes from one row's start to the next's.
    pub fn stride(&self) -> u32 {
        self.0.dmabuf.stride
    }

    /// Where its first row starts in the dmabuf.
    pub fn offset(&self) -> u32 {
        self.0.dmabuf.offset
    }

    /// The DRM format modifier its memory is laid out in.
    pub fn modifier(&self) -> u64 {
        self.0.dmabuf.modifier
    }

    pub(crate) fn dmabuf(&self) -> &Dmabuf {
        &self.0.dmabuf
    }
}

impl std::fmt::Debug for HardwareBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HardwareBuffer")
            .field("fd", &self.fd().as_raw_fd())
            .field("size", &self.size())
            .finish()
    }
}

/// One hardware buffer's memory, as the engine holds it: the dmabuf, which a program's device
/// draws into through a texture made over it for each frame, and which the shell is handed as the
/// [`HardwareBuffer`] it shows. Cloning it is a hold more: whoever still holds one keeps the
/// pixels alive.
#[derive(Clone)]
pub(crate) struct HardwareBufferMemory {
    buffer: HardwareBuffer,
    width: u32,
    height: u32,
}

impl HardwareBufferMemory {
    /// Allocates a BGRA8 buffer of `width` by `height` physical pixels; linear memory serves every
    /// `usage`.
    pub(crate) fn new(
        width: u32,
        height: u32,
        usage: HardwareBufferUsage,
    ) -> Result<HardwareBufferMemory> {
        let _ = usage;
        let dmabuf = Dmabuf::allocate(width, height)?;
        Ok(HardwareBufferMemory {
            buffer: HardwareBuffer::over(dmabuf),
            width,
            height,
        })
    }

    /// A program's buffer of `width` by `height` pixels in `format`, for `usage`, sides the
    /// program checked: `unsupported` for RGBA, which Linux's buffers are not allocated in, and
    /// `out-of-memory` where the system allocates none.
    pub(crate) fn allocate(
        width: u32,
        height: u32,
        format: PixelFormat,
        usage: HardwareBufferUsage,
    ) -> Result<HardwareBufferMemory, ImageError> {
        if format != PixelFormat::Bgra8 {
            return Err(ImageError {
                kind: ImageErrorKind::Unsupported,
                message: "Linux allocates hardware buffers in BGRA".to_owned(),
            });
        }
        HardwareBufferMemory::new(width, height, usage).map_err(|error| ImageError {
            kind: ImageErrorKind::OutOfMemory,
            message: format!("{error:#}"),
        })
    }

    /// The memory of `buffer`, a hardware buffer the shell handed over: one a layer shows.
    pub(crate) fn of(buffer: HardwareBuffer) -> HardwareBufferMemory {
        let [width, height] = buffer.size();
        HardwareBufferMemory {
            buffer,
            width,
            height,
        }
    }

    /// The hardware buffer as the shell shows it. A hold, not a copy.
    pub(crate) fn buffer(&self) -> HardwareBuffer {
        self.buffer.clone()
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// What the buffer can be used for: every use, linear memory serving all.
    pub(crate) fn usage(&self) -> HardwareBufferUsage {
        HardwareBufferUsage::all()
    }

    /// The bytes the buffer's rows take: what a program is counted for holding it.
    pub(crate) fn bytes(&self) -> u64 {
        u64::from(self.buffer.stride()) * u64::from(self.height)
    }

    /// The buffer's pixels copied into the program's memory, rows `width * 4` bytes, BGRA and
    /// premultiplied, read by the CPU.
    pub(crate) fn read_pixels(&self) -> Result<PixelBuffer, ImageError> {
        let bytes = self
            .buffer
            .dmabuf()
            .read_rows()
            .map_err(|error| ImageError {
                kind: ImageErrorKind::Other,
                message: format!("{error:#}"),
            })?;
        Ok(PixelBuffer {
            width: self.width,
            height: self.height,
            row_bytes: self.width * 4,
            format: PixelFormat::Bgra8,
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
            format: GpuTextureFormat::Bgra8unorm,
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

    /// What tells this buffer's screenshots apart from another's: its `AHardwareBuffer`, unique
    /// while anyone holds a reference on it, as a reader and a release waiting for one do.
    fn key(&self) -> usize {
        Arc::as_ptr(&self.buffer.0) as usize
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
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) -> Option<wgpu_core::SubmissionIndex> {
    submit_transfer(gpu, device, texture, handed_state(usage), Transfer::Release)
        .inspect_err(|error| {
            if !device.is_lost() {
                tracing::warn!("a frame is handed to the compositor unreleased: {error:#}");
            }
        })
        .ok()
}

/// Takes `texture`, a frame's over a hardware buffer, back from the compositor before the program
/// draws into it: its image is acquired from the compositor's queue family, and moved to the
/// state of `usage` a frame is handed over in. A failure, as on a lost device, is said, and the
/// frame drawn without it. GPU work, submitted only where the program's freeze allows it.
pub(crate) fn take_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    wait_for_readers(memory);
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
    memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    wait_for_readers(memory);
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

/// Waits until the fence the dmabuf's readers left, the compositor's among them, has signalled:
/// the sync file `DMA_BUF_IOCTL_EXPORT_SYNC_FILE` exports, polled for at most [`READERS_WAIT`].
/// The wait is on the CPU, on the program's thread: a semaphore imported from the sync file and
/// waited on by the next submission, the GPU's wait, crashes lavapipe's `vkQueueSubmit`, and the
/// fence has nearly always signalled by the time a buffer comes back (measured: usually signalled
/// before the wait, a few of 187 buffers with it still pending). Where it cannot be waited for,
/// it is said, and the buffer is drawn into without it.
fn wait_for_readers(memory: &HardwareBufferMemory) {
    if let Err(error) = readers_done(memory) {
        tracing::warn!("a buffer is drawn into without waiting for its readers: {error:#}");
    }
}

/// How long a buffer's readers are waited for before it is drawn into anyway.
const READERS_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

fn readers_done(memory: &HardwareBufferMemory) -> Result<()> {
    let fence = memory.buffer.dmabuf().writers_fence()?;
    let mut poll = libc::pollfd {
        fd: fence.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one live fd, polled for reading.
    let ready = unsafe { libc::poll(&mut poll, 1, READERS_WAIT.as_millis() as i32) };
    anyhow::ensure!(
        ready == 1,
        "its readers were not done within {READERS_WAIT:?}"
    );
    Ok(())
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

/// A video's frame as the engine holds it. Android hands out none yet, so here the type has no
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
