//! A hardware buffer on Linux: either a dmabuf a program's GPU device draws into and the
//! compositor shows with no copy between them, or shared memory a frame is read back into, which
//! the compositor maps, where it imports no dmabuf the engine's GPU makes and in a headless WaOS
//! (`BufferKind`).
//!
//! For the dmabuf path: GBM allocates it on the first render node, linear and exported read-write
//! (`dmabuf`), Vulkan imports its memory into an image in its modifier and plane layout
//! (`imported_image`), on a device opened to import it (`vulkan::device_opener`), and wgpu-core
//! takes the image as a texture of its own. As on Android, the compositor reads the buffer outside
//! Vulkan, so a frame's image is released to the foreign queue family once the work that drew it
//! is submitted and acquired back before it is drawn into again (`vulkan::queue_ownership`); and
//! before that acquire, the program's thread waits for the fence the compositor's reads left on
//! the dmabuf.
//!
//! For shared memory: plain `wgpu` BGRA8 textures are created on the program's device (no
//! `VK_EXT_external_memory_dma_buf` or `VK_EXT_queue_family_foreign` required, so Mesa lavapipe
//! and headless Linux hosts work without `/dev/dri/renderD*`). On handover, the texture is copied
//! into a staging buffer inside the freeze gate and read back into the buffer's shared memory
//! (`shared_memory`) on the finisher thread before handing the buffer to the shell.

// Wrapping the system's allocation into a texture, and handing its image between queue families,
// is what this module is for; the rest of the crate has no `unsafe` but where `code_folder` maps
// compiled code.
#![allow(unsafe_code)]

mod dmabuf;
mod imported_image;
mod shared_memory;

use std::borrow::Cow;
use std::collections::HashMap;
use std::convert::Infallible;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use waos_gpu_host::{Device, GpuObject, TextureDescriptor};

use crate::BufferKind;
use crate::bindings::wasdk::platform::hardware_buffers::{
    AlphaType, HardwareBufferFormat, HardwareBufferUsage, ImageError, ImageErrorKind, PixelBuffer,
    PixelFormat,
};
use crate::gpu::wgpu_types::error::{ErrorType, WebGpuError};
use crate::gpu::{GpuInstance, ProgramsAdapter, wgpu_core, wgpu_types};
use crate::hardware_buffers::Released;
use crate::os::{FinishHandOver, HandOver};
use crate::shell::{AcceptedBuffers, VideoPicture};
use crate::sync::locked;

pub(crate) use super::vulkan::device_opener::HardwareBufferImports;
use super::vulkan::device_opener::programs_adapter_imports;
pub(crate) use super::vulkan::device_opener::render_node as adapter_gpu;
use super::vulkan::queue_ownership::{Transfer, handed_state, submit_transfer};
use dmabuf::Dmabuf;
use imported_image::ImportedTexture;
use shared_memory::SharedMemory;

/// The format a hardware buffer is allocated in, and so the only one a canvas can be configured
/// with: the format a view's surface offers on this platform. Android's public formats have no BGRA.
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

/// Whether a texture over a hardware buffer is handed to the compositor and back: on Linux a
/// dmabuf's image is released to the foreign queue family and acquired back, and a texture's
/// pixels are copied into shared memory on handover.
pub(crate) const HANDS_TEXTURES_OVER: bool = true;

/// Row alignment required by WebGPU texture-to-buffer copies.
const COPY_ALIGN: u32 = wgpu_types::COPY_BYTES_PER_ROW_ALIGNMENT;

/// What programs' pictures are kept in, chosen once as the engine starts: dmabufs where the
/// compositor imports a linear BGRA8 one, GBM allocates one, and the programs' adapter imports it
/// and is on the GPU the compositor shows from;
/// shared memory otherwise, said once, and where there is no compositor to show them.
pub(crate) fn buffer_kind(
    gpu: &GpuInstance,
    accepted: Option<&AcceptedBuffers>,
    programs_adapter: &ProgramsAdapter,
) -> BufferKind {
    let Some(accepted) = accepted else {
        return BufferKind::SharedMemory;
    };
    if imports_linear_bgra(accepted)
        && Dmabuf::can_allocate()
        && programs_adapter_imports(gpu, programs_adapter)
    {
        return BufferKind::GpuMemory(accepted.gpu());
    }
    if accepted.shared_memory {
        tracing::info!(
            "programs' pictures are kept in shared memory: the compositor imports no dmabuf the \
             engine's GPU makes"
        );
    } else {
        tracing::warn!(
            "the compositor imports no dmabuf the engine's GPU makes and maps no shared memory: \
             programs' pictures are not shown"
        );
    }
    BufferKind::SharedMemory
}

/// Whether the compositor imports BGRA8 in the linear layout GBM allocates the engine's dmabufs
/// in: as `DRM_FORMAT_MOD_LINEAR`, or as the driver's implicit layout offered alone, which Waft
/// announces a linear buffer in.
fn imports_linear_bgra(accepted: &AcceptedBuffers) -> bool {
    accepted
        .gpu_memory
        .iter()
        .flat_map(|memory| &memory.formats)
        .any(|accepted| {
            accepted.format == HardwareBufferFormat::Bgra8
                && (accepted.modifiers.contains(&dmabuf::MOD_LINEAR)
                    || accepted.modifiers.contains(&dmabuf::MOD_INVALID))
        })
}

/// A picture's memory, which the system's compositor shows: a dmabuf, or shared memory (see the
/// module's docs). Cloning it is a hold more on the same backing.
#[derive(Clone)]
pub struct HardwareBuffer(Arc<LinuxBuffer>);

enum LinuxBuffer {
    Dmabuf(ShellsDmabuf),
    SharedMemory(SharedMemoryBuffer),
}

/// A dmabuf, and what the shell made of it once, which lives as long as the dmabuf: Waft's buffer
/// over it, which keeps the `wl_buffer` the compositor made of it, so a pool's buffer presented
/// every frame is made a `wl_buffer` once.
struct ShellsDmabuf {
    dmabuf: Dmabuf,
    shells: OnceLock<Arc<dyn std::any::Any + Send + Sync>>,
}

/// BGRA8 premultiplied rows of `width * 4` bytes in shared memory, and what the shell made of
/// them once, as for a dmabuf.
struct SharedMemoryBuffer {
    width: u32,
    height: u32,
    pixels: SharedMemory,
    shells: OnceLock<Arc<dyn std::any::Any + Send + Sync>>,
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
        HardwareBuffer(Arc::new(LinuxBuffer::Dmabuf(ShellsDmabuf {
            dmabuf,
            shells: OnceLock::new(),
        })))
    }

    fn shared_memory(width: u32, height: u32) -> Result<HardwareBuffer> {
        let pixels = SharedMemory::new(width as usize * height as usize * 4)?;
        Ok(HardwareBuffer(Arc::new(LinuxBuffer::SharedMemory(
            SharedMemoryBuffer {
                width,
                height,
                pixels,
                shells: OnceLock::new(),
            },
        ))))
    }

    /// Whether this buffer is shared memory rather than a dmabuf.
    pub fn is_shared_memory(&self) -> bool {
        matches!(self.0.as_ref(), LinuxBuffer::SharedMemory(_))
    }

    /// What the shell makes of the buffer, `make`'s answer, made at the first call and kept as
    /// long as the buffer is; `None` where an earlier call made something of another type.
    pub fn shells<T: std::any::Any + Send + Sync>(
        &self,
        make: impl FnOnce() -> T,
    ) -> Option<Arc<T>> {
        let shells = match self.0.as_ref() {
            LinuxBuffer::Dmabuf(shell_dmabuf) => &shell_dmabuf.shells,
            LinuxBuffer::SharedMemory(shared) => &shared.shells,
        };
        let made = shells.get_or_init(|| Arc::new(make()));
        Arc::clone(made).downcast::<T>().ok()
    }

    /// The fd of its memory, the dmabuf or the shared memory, which a reader duplicates rather
    /// than takes.
    pub fn fd(&self) -> &OwnedFd {
        match self.0.as_ref() {
            LinuxBuffer::Dmabuf(shell_dmabuf) => &shell_dmabuf.dmabuf.fd,
            LinuxBuffer::SharedMemory(shared) => shared.pixels.fd(),
        }
    }

    /// Its width and height in pixels.
    pub fn size(&self) -> [u32; 2] {
        match self.0.as_ref() {
            LinuxBuffer::Dmabuf(shell_dmabuf) => {
                [shell_dmabuf.dmabuf.width, shell_dmabuf.dmabuf.height]
            }
            LinuxBuffer::SharedMemory(shared) => [shared.width, shared.height],
        }
    }

    /// The bytes from one row's start to the next's.
    pub fn stride(&self) -> u32 {
        match self.0.as_ref() {
            LinuxBuffer::Dmabuf(shell_dmabuf) => shell_dmabuf.dmabuf.stride,
            LinuxBuffer::SharedMemory(shared) => shared.width * 4,
        }
    }

    /// Where its first row starts in the dmabuf; `None` for shared memory, whose rows start at
    /// its start.
    pub fn offset(&self) -> Option<u32> {
        Some(self.dmabuf()?.offset)
    }

    /// The DRM format modifier its memory is laid out in; `None` for shared memory.
    pub fn modifier(&self) -> Option<u64> {
        Some(self.dmabuf()?.modifier)
    }

    pub(crate) fn dmabuf(&self) -> Option<&Dmabuf> {
        match self.0.as_ref() {
            LinuxBuffer::Dmabuf(shell_dmabuf) => Some(&shell_dmabuf.dmabuf),
            LinuxBuffer::SharedMemory(_) => None,
        }
    }
}

impl std::fmt::Debug for HardwareBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.as_ref() {
            LinuxBuffer::Dmabuf(shell_dmabuf) => f
                .debug_struct("HardwareBuffer")
                .field("fd", &shell_dmabuf.dmabuf.fd.as_raw_fd())
                .field("size", &self.size())
                .finish(),
            LinuxBuffer::SharedMemory(shared) => f
                .debug_struct("HardwareBuffer")
                .field("shared_memory", &shared.pixels.fd().as_raw_fd())
                .field("size", &self.size())
                .finish(),
        }
    }
}

/// One hardware buffer's memory, as the engine holds it: either a dmabuf or shared memory, which a program's device draws into through a texture made over it for each frame, and
/// which the shell is handed as the [`HardwareBuffer`] it shows. Cloning it is a hold more:
/// whoever still holds one keeps the pixels alive.
#[derive(Clone)]
pub(crate) struct HardwareBufferMemory {
    buffer: HardwareBuffer,
    width: u32,
    height: u32,
}

impl HardwareBufferMemory {
    /// Allocates a BGRA8 buffer of `width` by `height` physical pixels in `mode`; linear memory
    /// and shared memory serve every `usage`.
    pub(crate) fn new(
        width: u32,
        height: u32,
        usage: HardwareBufferUsage,
        mode: BufferKind,
    ) -> Result<HardwareBufferMemory> {
        let _ = usage;
        let buffer = match mode {
            BufferKind::GpuMemory(_) => HardwareBuffer::over(Dmabuf::allocate(width, height)?),
            BufferKind::SharedMemory => HardwareBuffer::shared_memory(width, height)?,
        };
        Ok(HardwareBufferMemory {
            buffer,
            width,
            height,
        })
    }

    /// Why a buffer in `format` for `usage` is not made here; none where it is: Linux's buffers
    /// are allocated in BGRA, linear or in shared memory, for every use.
    pub(crate) fn unsupported(
        format: HardwareBufferFormat,
        usage: HardwareBufferUsage,
    ) -> Option<&'static str> {
        let _ = usage;
        (format != HardwareBufferFormat::Bgra8)
            .then_some("Linux allocates hardware buffers in BGRA")
    }

    /// A program's buffer of `width` by `height` pixels in `format`, for `usage`, sides the
    /// program checked: `unsupported` for any format but BGRA, which Linux's buffers are
    /// allocated in, and `out-of-memory` where the system allocates none.
    pub(crate) fn allocate(
        width: u32,
        height: u32,
        format: HardwareBufferFormat,
        usage: HardwareBufferUsage,
        mode: BufferKind,
    ) -> Result<HardwareBufferMemory, ImageError> {
        if let Some(message) = HardwareBufferMemory::unsupported(format, usage) {
            return Err(ImageError {
                kind: ImageErrorKind::Unsupported,
                message: message.to_owned(),
            });
        }
        HardwareBufferMemory::new(width, height, usage, mode).map_err(|error| ImageError {
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

    /// What the buffer can be used for: every use, linear memory and shared memory serving
    /// all.
    pub(crate) fn usage(&self) -> HardwareBufferUsage {
        HardwareBufferUsage::all()
    }

    /// How the buffer lays out its pixels: BGRA, as every dmabuf here is ARGB8888, the engine's
    /// and one another host made ([`HardwareBuffer::new`]), and shared memory BGRA8 rows. Never a
    /// failure here.
    pub(crate) fn format(&self) -> Result<HardwareBufferFormat> {
        Ok(HardwareBufferFormat::Bgra8)
    }

    /// The bytes the buffer's rows take: what a program is counted for holding it.
    pub(crate) fn bytes(&self) -> u64 {
        let row_bytes = self.buffer.stride();
        u64::from(row_bytes) * u64::from(self.height)
    }

    /// The buffer's pixels copied into the program's memory, rows `width * 4` bytes, BGRA and
    /// premultiplied, read by the CPU.
    pub(crate) fn read_pixels(&self) -> Result<PixelBuffer, ImageError> {
        let bytes = match self.buffer.0.as_ref() {
            LinuxBuffer::Dmabuf(shell_dmabuf) => {
                shell_dmabuf
                    .dmabuf
                    .read_rows()
                    .map_err(|error| ImageError {
                        kind: ImageErrorKind::Other,
                        message: format!("{error:#}"),
                    })?
            }
            LinuxBuffer::SharedMemory(shared) => shared.pixels.bytes().to_vec(),
        };
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
    ) -> Result<(Arc<GpuObject<wgpu_core::id::TextureId>>, TextureDescriptor)> {
        let size = [self.width, self.height];
        let texture = match self.buffer.0.as_ref() {
            LinuxBuffer::Dmabuf(_) => {
                let imported = ImportedTexture {
                    label: "waos program hardware buffer",
                    usage,
                    view_formats: &[],
                    arrives: arriving_state(usage),
                };
                imported.over(&self.buffer, size, gpu, device.device_id(), Some(release))?
            }
            LinuxBuffer::SharedMemory(shared) => {
                // Plain wgpu textures own their own GPU memory rather than referencing external
                // memory, so no external drop callback is needed once the texture is created.
                drop(release);
                let actual_usage = usage
                    | wgpu_types::TextureUsages::COPY_SRC
                    | wgpu_types::TextureUsages::COPY_DST;
                let texture = create_offscreen_texture(
                    gpu,
                    device.device_id(),
                    size,
                    "waos program hardware buffer",
                    actual_usage,
                    &[],
                )?;
                if !device.is_lost() {
                    upload_shared_pixels(gpu, device.queue_id(), texture, shared)?;
                }
                texture
            }
        };
        let descriptor =
            crate::gpu::hardware_buffer_texture(size, wgpu_types::TextureFormat::Bgra8Unorm, usage);
        Ok((Arc::new(GpuObject::new(texture, gpu)), descriptor))
    }

    /// A texture over the buffer's memory on `device` of `gpu`, which the GPU samples, its pixels
    /// taken as written: a picture a screenshot reads. A failure the system answers is the
    /// engine's.
    pub(crate) fn sampled_texture(
        &self,
        gpu: &wgpu_core::global::Global,
        device: wgpu_core::id::DeviceId,
        queue: wgpu_core::id::QueueId,
    ) -> Result<wgpu_core::id::TextureId> {
        match self.buffer.0.as_ref() {
            LinuxBuffer::Dmabuf(_) => {
                let sampled = ImportedTexture {
                    label: "waos screenshot picture",
                    usage: wgpu_types::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                    arrives: wgpu_types::TextureUses::RESOURCE,
                };
                sampled.over(&self.buffer, [self.width, self.height], gpu, device, None)
            }
            LinuxBuffer::SharedMemory(shared) => {
                let texture = create_offscreen_texture(
                    gpu,
                    device,
                    [self.width, self.height],
                    "waos screenshot picture",
                    wgpu_types::TextureUsages::TEXTURE_BINDING
                        | wgpu_types::TextureUsages::COPY_DST,
                    &[],
                )?;
                upload_shared_pixels(gpu, queue, texture, shared)?;
                Ok(texture)
            }
        }
    }

    /// The buffer marked as read by a screenshot until what is answered goes: a release the engine
    /// handed the shell with it (`after_reads`) waits for it meanwhile.
    pub(crate) fn in_use(&self) -> InUse {
        locked(&READS).entry(self.key()).or_default().readers += 1;
        InUse(self.clone())
    }

    /// `released`, a release the engine hands the shell with the buffer, run once no screenshot
    /// reads the buffer: at once where none does, or as the last of them is done.
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
        match self.buffer.0.as_ref() {
            LinuxBuffer::Dmabuf(_) => {
                let drawn = ImportedTexture {
                    label: "waos hardware buffer",
                    usage,
                    view_formats,
                    // A new image over the buffer, holding the last frame's pixels, which nothing
                    // reads: the program draws the whole canvas.
                    arrives: wgpu_types::TextureUses::UNINITIALIZED,
                };
                drawn.over(&self.buffer, [self.width, self.height], gpu, device, None)
            }
            LinuxBuffer::SharedMemory(_) => create_offscreen_texture(
                gpu,
                device,
                [self.width, self.height],
                "waos hardware buffer",
                usage | wgpu_types::TextureUsages::COPY_SRC,
                view_formats,
            ),
        }
    }

    /// Encodes a copy from `texture` into a staging buffer on `encoder` when this buffer is
    /// shared memory, returning the post-wait closure that maps the staging buffer and writes the
    /// shared memory. Returns `Ok(None)` for dmabufs.
    pub(crate) fn encode_readback(
        &self,
        gpu: &GpuInstance,
        device: wgpu_core::id::DeviceId,
        encoder: wgpu_core::id::CommandEncoderId,
        texture: wgpu_core::id::TextureId,
    ) -> Result<Option<FinishHandOver>> {
        if !self.buffer.is_shared_memory() {
            return Ok(None);
        }
        let width = self.width;
        let height = self.height;
        if width == 0 || height == 0 {
            return Ok(None);
        }
        let padded_row = padded_bytes_per_row(width);
        let total_bytes = u64::from(padded_row) * u64::from(height);
        let (staging_id, error) = gpu.device_create_buffer(
            device,
            &wgpu_types::BufferDescriptor {
                label: Some(Cow::Borrowed("waos offscreen readback")),
                size: total_bytes,
                usage: wgpu_types::BufferUsages::COPY_DST | wgpu_types::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            },
            None,
        );
        let staging = GpuObject::new(staging_id, gpu);
        if let Some(error) = error {
            return Err(anyhow!("{error}")).context("creating offscreen readback staging buffer");
        }
        gpu.command_encoder_copy_texture_to_buffer(
            encoder,
            &wgpu_types::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu_types::Origin3d::ZERO,
                aspect: wgpu_types::TextureAspect::All,
            },
            &wgpu_types::TexelCopyBufferInfo {
                buffer: staging.id(),
                layout: wgpu_types::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            &wgpu_types::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        )
        .context("copying offscreen texture to staging buffer")?;
        let buffer = self.buffer.clone();
        let finish: FinishHandOver = Box::new(move |gpu| {
            finish_readback(gpu, device, staging, &buffer, width, height, padded_row);
        });
        Ok(Some(finish))
    }

    /// What tells this buffer's screenshots apart from another's: its backing pointer, unique
    /// while anyone holds a reference on it, as a reader and a release waiting for one do.
    fn key(&self) -> usize {
        Arc::as_ptr(&self.buffer.0) as usize
    }
}

/// Hands a frame the program drew into `texture`, over a hardware buffer, to the compositor once
/// its work is done. For dmabufs, releases the image to the foreign queue family; for shared memory
/// buffers, encodes and submits the staging-buffer copy inside the freeze gate and returns the
/// post-wait readback closure.
pub(crate) fn hand_to_compositor(
    gpu: &GpuInstance,
    device: &Device,
    memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) -> HandOver {
    match memory.buffer.0.as_ref() {
        LinuxBuffer::Dmabuf(_) => {
            let submission =
                submit_transfer(gpu, device, texture, handed_state(usage), Transfer::Release)
                    .inspect_err(|error| {
                        if !device.is_lost() {
                            tracing::warn!(
                                "a frame is handed to the compositor unreleased: {error:#}"
                            );
                        }
                    })
                    .ok();
            HandOver::submission(submission)
        }
        LinuxBuffer::SharedMemory(_) => {
            if device.is_lost() {
                return HandOver::submission(None);
            }
            match submit_readback(gpu, device, memory, texture) {
                Ok(hand_over) => hand_over,
                Err(error) => {
                    if !device.is_lost() {
                        tracing::warn!("a frame was not read back: {error:#}");
                    }
                    HandOver::submission(device.submit_pending_writes())
                }
            }
        }
    }
}

/// Takes `texture`, a frame's over a hardware buffer, back from the compositor before the program
/// draws into it: acquires from the foreign queue family for dmabufs, and is a no-op for shared
/// buffers.
pub(crate) fn take_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    if memory.buffer.is_shared_memory() {
        return;
    }
    wait_for_readers(memory);
    let taken = submit_transfer(gpu, device, texture, handed_state(usage), Transfer::Acquire);
    if let Err(error) = taken
        && !device.is_lost()
    {
        tracing::warn!("a frame is drawn without taking it back from the compositor: {error:#}");
    }
}

/// Takes `texture`, a program's over its own hardware buffer, back for the program's device once
/// the compositor, the CPU or the codecs are done with the buffer: acquires back for dmabufs, and
/// is a no-op for shared memory.
pub(crate) fn take_back_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    if memory.buffer.is_shared_memory() {
        return;
    }
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

/// Encodes and submits an offscreen texture-to-staging-buffer copy on `device`'s queue.
fn submit_readback(
    gpu: &GpuInstance,
    device: &Device,
    memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
) -> Result<HandOver> {
    let label = Some(Cow::Borrowed("waos offscreen handover"));
    let (encoder_id, error) = gpu.device_create_command_encoder(
        device.device_id(),
        &wgpu_types::CommandEncoderDescriptor {
            label: label.clone(),
        },
        None,
    );
    let encoder = GpuObject::new(encoder_id, gpu);
    if let Some(error) = error {
        return Err(anyhow!("{error}")).context("creating offscreen handover command encoder");
    }
    let finish = memory.encode_readback(gpu, device.device_id(), encoder.id(), texture)?;
    let (commands_id, error) = gpu.command_encoder_finish(
        encoder.id(),
        &wgpu_types::CommandBufferDescriptor { label },
        None,
    );
    let commands = GpuObject::new(commands_id, gpu);
    if let Some((_, error)) = error {
        return Err(anyhow!("{error}")).context("finishing offscreen handover command buffer");
    }
    let submission = gpu
        .queue_submit(device.queue_id(), &[commands.id()])
        .map_err(|(_, error)| anyhow!("{error}"))
        .context("submitting offscreen handover copy")?;
    Ok(HandOver {
        submission: Some(submission),
        finish,
    })
}

/// Maps `staging` after its copy submission has completed and copies its rows into `buffer`'s
/// shared memory, stripping WebGPU row padding.
fn finish_readback(
    gpu: &wgpu_core::global::Global,
    device: wgpu_core::id::DeviceId,
    staging: GpuObject<wgpu_core::id::BufferId>,
    buffer: &HardwareBuffer,
    width: u32,
    height: u32,
    padded_row: u32,
) {
    let LinuxBuffer::SharedMemory(shared) = buffer.0.as_ref() else {
        return;
    };
    let total_bytes = u64::from(padded_row) * u64::from(height);
    let mapped = Arc::new(Mutex::new(None));
    let mapped_cb = Arc::clone(&mapped);
    let op = wgpu_core::resource::BufferMapOperation {
        host: wgpu_core::device::HostMap::Read,
        callback: Some(Box::new(move |result| {
            *locked(&mapped_cb) = Some(result);
        })),
    };
    if let Err(error) = gpu.buffer_map_async(staging.id(), 0, Some(total_bytes), op) {
        tracing::warn!("mapping offscreen readback buffer: {error}");
        return;
    }
    if let Err(error) = gpu.device_poll(device, wgpu_types::PollType::wait_indefinitely()) {
        tracing::warn!("polling offscreen readback buffer: {error:?}");
        return;
    }
    match locked(&mapped).take() {
        Some(Ok(())) => {
            match gpu.buffer_get_mapped_range(staging.id(), 0, Some(total_bytes)) {
                Ok((ptr, len)) => {
                    let unpadded_row = width as usize * 4;
                    let padded_row = padded_row as usize;
                    let needed = padded_row * height as usize;
                    if len as usize >= needed {
                        // SAFETY: `buffer_get_mapped_range` succeeded for `len >= needed` bytes
                        // and the buffer stays mapped until `buffer_unmap` below.
                        let src = unsafe { std::slice::from_raw_parts(ptr.as_ptr(), len as usize) };
                        let mut pixels = shared.pixels.bytes();
                        for row in 0..height as usize {
                            let src_start = row * padded_row;
                            let dst_start = row * unpadded_row;
                            pixels[dst_start..dst_start + unpadded_row]
                                .copy_from_slice(&src[src_start..src_start + unpadded_row]);
                        }
                    }
                    let _ = gpu.buffer_unmap(staging.id());
                }
                Err(error) => {
                    tracing::warn!("reading mapped offscreen buffer: {error}");
                }
            }
        }
        Some(Err(error)) => {
            tracing::warn!("offscreen readback map failed: {error}");
        }
        None => {
            tracing::warn!("offscreen readback map callback did not fire");
        }
    }
    drop(staging);
}

fn padded_bytes_per_row(width: u32) -> u32 {
    let unpadded = width * 4;
    unpadded.div_ceil(COPY_ALIGN) * COPY_ALIGN
}

fn create_offscreen_texture(
    gpu: &wgpu_core::global::Global,
    device: wgpu_core::id::DeviceId,
    [width, height]: [u32; 2],
    label: &'static str,
    usage: wgpu_types::TextureUsages,
    view_formats: &[wgpu_types::TextureFormat],
) -> Result<wgpu_core::id::TextureId> {
    let descriptor = wgpu_types::TextureDescriptor {
        label: Some(Cow::Borrowed(label)),
        size: wgpu_types::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu_types::TextureDimension::D2,
        format: FORMAT,
        usage,
        view_formats: view_formats.to_vec(),
    };
    let (texture, error) = gpu.device_create_texture(device, &descriptor, None);
    match error {
        Some(error) if error.webgpu_error_type() != ErrorType::DeviceLost => {
            bail!("{error:#?}")
        }
        _ => Ok(texture),
    }
}

fn upload_shared_pixels(
    gpu: &wgpu_core::global::Global,
    queue: wgpu_core::id::QueueId,
    texture: wgpu_core::id::TextureId,
    shared: &SharedMemoryBuffer,
) -> Result<()> {
    let pixels = shared.pixels.bytes();
    let expected = shared.width as usize * shared.height as usize * 4;
    if pixels.len() < expected || expected == 0 {
        return Ok(());
    }
    gpu.queue_write_texture(
        queue,
        &wgpu_types::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu_types::Origin3d::ZERO,
            aspect: wgpu_types::TextureAspect::All,
        },
        &pixels[..expected],
        &wgpu_types::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(shared.width * 4),
            rows_per_image: Some(shared.height),
        },
        &wgpu_types::Extent3d {
            width: shared.width,
            height: shared.height,
            depth_or_array_layers: 1,
        },
    )
    .map_err(|error| anyhow!("uploading offscreen pixels: {error}"))
}

/// Waits until the fence the dmabuf's readers left, the compositor's among them, has signalled:
/// the sync file `DMA_BUF_IOCTL_EXPORT_SYNC_FILE` exports. A buffer is never drawn into while a
/// reader still reads it, which would show a frame half drawn; the GPU work a reader left always
/// ends, or its device is lost, which signals the fence too.
fn wait_for_readers(memory: &HardwareBufferMemory) {
    if let Err(error) = readers_done(memory) {
        tracing::warn!("a buffer's readers could not be waited for: {error:#}");
    }
}

fn readers_done(memory: &HardwareBufferMemory) -> Result<()> {
    let Some(dmabuf) = memory.buffer.dmabuf() else {
        return Ok(());
    };
    let fence = dmabuf.writers_fence()?;
    let mut poll = libc::pollfd {
        fd: fence.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: one live fd, polled for reading.
        if unsafe { libc::poll(&mut poll, 1, -1) } == 1 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error).context("polling its readers' fence");
        }
    }
}

/// A buffer of `width` by `height` pixels, each the bytes `pixel` answers for its column and row
/// as they lie in memory, BGRA: a picture a test knows.
#[cfg(test)]
pub(crate) fn filled(
    width: u32,
    height: u32,
    pixel: impl Fn(u32, u32) -> [u8; 4],
) -> HardwareBufferMemory {
    let memory = HardwareBufferMemory::new(
        width,
        height,
        HardwareBufferUsage::empty(),
        BufferKind::SharedMemory,
    )
    .expect("shared memory for a test");
    if let LinuxBuffer::SharedMemory(shared) = memory.buffer.0.as_ref() {
        let mut pixels = shared.pixels.bytes();
        for row in 0..height {
            for column in 0..width {
                let start = (row * width + column) as usize * 4;
                pixels[start..start + 4].copy_from_slice(&pixel(column, row));
            }
        }
    }
    memory
}

/// The screenshots reading each hardware buffer now, by its key, and the releases waiting for
/// them: the use count Linux does not keep.
static READS: LazyLock<Mutex<HashMap<usize, Reads>>> = LazyLock::new(Mutex::default);

/// One buffer's readers, and the releases that wait for the last of them.
#[derive(Default)]
struct Reads {
    readers: usize,
    waiting: Vec<Released>,
}

/// A hardware buffer read by a screenshot, until this goes.
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

/// A video's frame as the engine holds it. Linux hands out none yet, so here the type has no
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
