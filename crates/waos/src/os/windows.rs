//! A D3D12 texture a program's GPU device draws into and the shell shows, shared by an NT
//! handle, and the texture over it on the program's wgpu-core device. Unlike `IOSurfaceCreate` and
//! `AHardwareBuffer_allocate`, D3D12 makes a shared texture only on a device, so the engine keeps
//! a device of its own for it, on the adapter a program gets (`shared_texture`); every device on
//! that adapter opens the handle over the same memory. The shell copies each frame it shows into
//! a composition swapchain of the layer's own, on Waft's device, and releases the buffer once
//! that copy is done (Waft's `os/windows/composition.rs`).
//!
//! D3D12 tracks a resource's state per device, and another device's use of it starts from the
//! common state, so a frame's texture is moved to `COMMON` once the program has drawn it
//! (`hand_to_compositor`), as Dawn resets a shared texture at each access; every texture the
//! engine makes over a buffer arrives in that state.
//!
//! The invariant: a texture over a hardware buffer leaves the program's device only in the common
//! state, and the program draws into a buffer only once its release says the copy is done.

// Wrapping D3D12's shared textures into wgpu-core's textures is what this module is for; the rest
// of the crate has no `unsafe` but where `code_cache` maps compiled code and `files` makes
// Windows' file calls.
#![allow(unsafe_code)]

mod shared_texture;

use std::collections::HashMap;
use std::convert::Infallible;
use std::os::windows::io::{AsRawHandle as _, OwnedHandle};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use anyhow::{Context as _, Result, anyhow};
use waos_gpu_host::{Device, GpuObject, TextureDescriptor};

use crate::BufferKind;
use crate::bindings::wasdk::platform::hardware_buffers::{
    HardwareBufferFormat, HardwareBufferUsage, ImageError, ImageErrorKind, PixelBuffer,
};
use crate::gpu::{GpuInstance, wgpu_core, wgpu_types};
use crate::hardware_buffers::Released;
use crate::os::{FinishHandOver, HandOver};
use crate::shell::VideoPicture;
use crate::sync::locked;

use shared_texture::Opened;

/// The format a hardware buffer is made in, and so the only one a canvas can be configured with:
/// BGRA, as on Apple's systems and Linux.
pub(crate) const FORMAT: wgpu_types::TextureFormat = wgpu_types::TextureFormat::Bgra8Unorm;

/// Whether a program allocates hardware buffers on this system.
pub(crate) const ALLOCATES: bool = true;

/// The largest side of a hardware buffer a program allocates, in pixels: D3D12's
/// `D3D12_REQ_TEXTURE2D_U_OR_V_DIMENSION`.
pub(crate) const LARGEST_SIDE: u32 = 16384;

/// Whether a texture over a hardware buffer is handed to the compositor and back: on Windows it
/// is moved to the common state another device's use starts from.
pub(crate) const HANDS_TEXTURES_OVER: bool = true;

/// The LUID of `adapter`'s DXGI adapter, where it is D3D12's.
pub(crate) fn adapter_gpu(
    instance: &wgpu_core::global::Global,
    adapter: wgpu_core::id::AdapterId,
) -> Option<crate::shell::GpuId> {
    // SAFETY: the adapter is only asked for its description, and let go before this returns.
    let luid = unsafe {
        let hal = instance.adapter_as_hal::<wgpu_hal::api::Dx12>(adapter)?;
        hal.raw_adapter().GetDesc1().ok()?.AdapterLuid
    };
    Some(crate::shell::GpuId::Luid {
        low: luid.LowPart,
        high: luid.HighPart,
    })
}

/// GPU memory the system's compositor can show, as the shell hands it and is handed it: a D3D12
/// texture's NT handle, B8G8R8A8, which Waft's Windows host opens. Cloning it is a hold more on
/// the same handle.
#[derive(Clone)]
pub struct HardwareBuffer(Arc<SharedHandle>);

/// A shared texture's handle, and what the shell made of it once, which lives as long as the
/// handle: Waft's buffer over it, which keeps the texture opened on Waft's device, so a pool's
/// buffer presented every frame is opened there once.
struct SharedHandle {
    handle: OwnedHandle,
    width: u32,
    height: u32,
    shells: OnceLock<Arc<dyn std::any::Any + Send + Sync>>,
}

impl HardwareBuffer {
    /// A texture another host shared: its NT `handle`, `size` pixels in B8G8R8A8.
    pub fn new(handle: OwnedHandle, size: [u32; 2]) -> HardwareBuffer {
        HardwareBuffer(Arc::new(SharedHandle {
            handle,
            width: size[0],
            height: size[1],
            shells: OnceLock::new(),
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

    /// The texture's NT handle, which a reader duplicates rather than takes.
    pub fn handle(&self) -> &OwnedHandle {
        &self.0.handle
    }

    /// Its width and height in pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.0.width, self.0.height]
    }
}

impl std::fmt::Debug for HardwareBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HardwareBuffer")
            .field("handle", &self.0.handle.as_raw_handle())
            .field("size", &self.size())
            .finish()
    }
}

/// One hardware buffer's memory, as the engine holds it: the shared texture, which a program's
/// device draws into through a texture made over it for each frame, and which the shell is handed
/// as the [`HardwareBuffer`] it shows. Cloning it is a hold more.
#[derive(Clone)]
pub(crate) struct HardwareBufferMemory {
    buffer: HardwareBuffer,
}

impl HardwareBufferMemory {
    /// Makes a BGRA8 texture of `width` by `height` physical pixels, shared by NT handle, on the
    /// engine's own device; every GPU `usage` is served.
    pub(crate) fn new(
        width: u32,
        height: u32,
        usage: HardwareBufferUsage,
        mode: BufferKind,
    ) -> Result<HardwareBufferMemory> {
        let _ = usage;
        let gpu = match mode {
            BufferKind::GpuMemory(gpu) => gpu,
            BufferKind::SharedMemory => None,
        };
        let handle = shared_texture::make([width, height], gpu)?;
        Ok(HardwareBufferMemory {
            buffer: HardwareBuffer::new(handle, [width, height]),
        })
    }

    /// Why a buffer in `format` for `usage` is not made here; none where it is: Windows makes
    /// shared textures in BGRA, which the CPU does not read.
    pub(crate) fn unsupported(
        format: HardwareBufferFormat,
        usage: HardwareBufferUsage,
    ) -> Option<&'static str> {
        if format != HardwareBufferFormat::Bgra8 {
            Some("Windows makes hardware buffers in BGRA")
        } else if usage.contains(HardwareBufferUsage::CPU_READ) {
            Some("a hardware buffer on Windows is not read by the CPU")
        } else {
            None
        }
    }

    /// A program's buffer of `width` by `height` pixels in `format`, for `usage`, sides the
    /// program checked: `unsupported` for any format but BGRA, which Windows' buffers are made in,
    /// and for a buffer the CPU reads, which a shared texture is not; `out-of-memory` where none
    /// is made.
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
        HardwareBufferMemory { buffer }
    }

    /// The hardware buffer as the shell shows it. A hold, not a copy.
    pub(crate) fn buffer(&self) -> HardwareBuffer {
        self.buffer.clone()
    }

    pub(crate) fn width(&self) -> u32 {
        self.buffer.0.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.buffer.0.height
    }

    /// What the buffer can be used for: every GPU use; not the CPU's reads, as a shared texture
    /// has no mapping.
    pub(crate) fn usage(&self) -> HardwareBufferUsage {
        HardwareBufferUsage::GPU_SAMPLED
            | HardwareBufferUsage::GPU_COLOR_OUTPUT
            | HardwareBufferUsage::COMPOSER_OVERLAY
    }

    /// How the buffer lays out its pixels: BGRA, the DXGI format every shared texture here is in,
    /// `B8G8R8A8_UNORM` as the engine makes one (`shared_texture::make`) and as
    /// [`HardwareBuffer::new`] takes another host's. Never a failure here.
    pub(crate) fn format(&self) -> Result<HardwareBufferFormat> {
        Ok(HardwareBufferFormat::Bgra8)
    }

    /// The bytes the buffer's pixels take: what a program is counted for holding it.
    pub(crate) fn bytes(&self) -> u64 {
        u64::from(self.width()) * u64::from(self.height()) * 4
    }

    /// The CPU does not read a shared texture: its usage has no `cpu-read`, which the caller
    /// refuses before asking (`hardware_buffers::program_buffer`).
    pub(crate) fn read_pixels(&self) -> Result<PixelBuffer, ImageError> {
        Err(ImageError {
            kind: ImageErrorKind::Unsupported,
            message: "a hardware buffer on Windows is not read by the CPU".to_owned(),
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
        release: Released,
    ) -> Result<(Arc<GpuObject<wgpu_core::id::TextureId>>, TextureDescriptor)> {
        let opened = Opened {
            label: "waos program hardware buffer",
            usage,
            view_formats: &[],
        };
        let size = self.buffer.size();
        let texture = opened.over(&self.buffer, gpu, device.device_id(), Some(release))?;
        let descriptor =
            crate::gpu::hardware_buffer_texture(size, wgpu_types::TextureFormat::Bgra8Unorm, usage);
        Ok((Arc::new(GpuObject::new(texture, gpu)), descriptor))
    }

    /// A texture over the buffer's memory on `device` of `gpu`, which the GPU samples: a picture
    /// a screenshot reads. A failure the system answers is the engine's.
    pub(crate) fn sampled_texture(
        &self,
        gpu: &wgpu_core::global::Global,
        device: wgpu_core::id::DeviceId,
        _queue: wgpu_core::id::QueueId,
    ) -> Result<wgpu_core::id::TextureId> {
        let sampled = Opened {
            label: "waos screenshot picture",
            usage: wgpu_types::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        sampled.over(&self.buffer, gpu, device, None)
    }

    /// The buffer marked as read by a screenshot until what is answered goes: a release the engine
    /// handed the shell with it (`after_reads`) waits for it meanwhile.
    pub(crate) fn in_use(&self) -> InUse {
        locked(&READS).entry(self.key()).or_default().readers += 1;
        InUse(self.clone())
    }

    /// `released`, a release the engine hands the shell with the buffer, run once no screenshot
    /// reads the buffer: at once where none does, or as the last of them is done. The shell's
    /// release comes when its copy is done, which knows nothing of the engine's readers.
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
    /// program lets go of it once the frame is presented, while the buffer underneath outlives
    /// every one of them.
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
        let drawn = Opened {
            label: "waos hardware buffer",
            usage,
            view_formats,
        };
        drawn.over(&self.buffer, gpu, device, None)
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

    /// What tells this buffer's screenshots apart from another's: its shared handle, unique while
    /// anyone holds it, as a reader and a release waiting for one do.
    fn key(&self) -> usize {
        Arc::as_ptr(&self.buffer.0) as usize
    }
}

/// Hands a frame the program drew into `texture`, over a hardware buffer, to the compositor once
/// its work is done: what the program wrote on `device`'s queue and no submission carried is
/// submitted, with the texture moved to the common state another device's use starts from.
/// Answers that submission, after every one that drew the frame, for the wait before the
/// hand-over; none where wgpu-core refused it, as on a lost device, said so. GPU work, submitted
/// only where the program's freeze allows it.
pub(crate) fn hand_to_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) -> HandOver {
    let _ = usage;
    let submission = submit_move_to_common(gpu, device, texture)
        .inspect_err(|error| {
            if !device.is_lost() {
                tracing::warn!("a frame is handed to the compositor in its last state: {error:#}");
            }
        })
        .ok();
    HandOver::submission(submission)
}

/// Takes a frame's `texture` back from the compositor: nothing to do, as a frame's texture is new
/// each frame and arrives in the common state the compositor left it in.
pub(crate) fn take_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    let _ = (gpu, device, texture, usage);
}

/// Takes a program's `texture` back from the compositor: nothing to do, as wgpu-core knows it in
/// the common state `hand_to_compositor` left it in, and moves it out of it at its next use.
pub(crate) fn take_back_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    let _ = (gpu, device, texture, usage);
}

/// Submits on `device`'s queue a command buffer that moves `texture` to the common state, which
/// is `PRESENT` to wgpu-hal on D3D12.
fn submit_move_to_common(
    gpu: &GpuInstance,
    device: &Device,
    texture: wgpu_core::id::TextureId,
) -> Result<wgpu_core::SubmissionIndex> {
    let label = Some(std::borrow::Cow::Borrowed("waos hardware buffer handover"));
    let (encoder, error) = gpu.device_create_command_encoder(
        device.device_id(),
        &wgpu_types::CommandEncoderDescriptor {
            label: label.clone(),
        },
        None,
    );
    let encoder = GpuObject::new(encoder, gpu);
    if let Some(error) = error {
        return Err(anyhow!("{error}")).context("a hardware buffer's handover encoder");
    }
    gpu.command_encoder_transition_resources(
        encoder.id(),
        std::iter::empty(),
        std::iter::once(wgpu_types::TextureTransition {
            texture,
            selector: None,
            state: wgpu_types::TextureUses::PRESENT,
        }),
    )
    .context("moving a hardware buffer's texture to the common state")?;
    let (commands, error) = gpu.command_encoder_finish(
        encoder.id(),
        &wgpu_types::CommandBufferDescriptor { label },
        None,
    );
    let commands = GpuObject::new(commands, gpu);
    if let Some((_, error)) = error {
        return Err(anyhow!("{error}")).context("a hardware buffer's handover");
    }
    gpu.queue_submit(device.queue_id(), &[commands.id()])
        .map_err(|(_, error)| anyhow!("{error}"))
        .context("submitting a hardware buffer's handover")
}

/// The screenshots reading each hardware buffer now, by its key, and the releases waiting for
/// them: the use count the shell's copy does not keep.
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

/// A video's frame as the engine holds it. Windows hands out none yet, so here the type has no
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
