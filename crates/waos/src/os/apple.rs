//! An `IOSurface` a program's WebGPU device draws into and the compositor shows, with the same
//! bytes under both, and the texture over it on the program's wgpu-core device: Metal wraps the
//! surface, and wgpu-core takes the Metal texture as one of its own. A drawn frame needs nothing
//! more to reach the compositor: Metal and Core Animation share the surface without a transfer.
//! And a program's own hardware buffers, `IOSurface`s it allocates or Image I/O decoded into, and a
//! video frame's, the `IOSurface` its `CVPixelBuffer` is on, each drawn or sampled the same way,
//! through a texture over the surface handed to the program as its `gpu-texture`, and read while
//! the surface is locked. A screenshot samples a view's pictures through textures over their
//! surfaces on a device of its own, marking a picture's surface in use while it reads it.
//!
//! An `IOSurface` is the only Apple allocation two independent `MTLDevice`s can each wrap without
//! a copy, so it is what crosses between the program's device and the shell's: the engine makes
//! a texture on the program's device over it, and the shell makes a layer's contents of it.

// Wrapping the system's allocation into a texture is what this module is for; the rest of the
// crate has no `unsafe` but where `code_cache` maps compiled code.
#![allow(unsafe_code)]

use std::ptr;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};
use objc2_core_video::CVPixelBufferGetIOSurface;
use objc2_io_surface::{
    IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceBytesPerRow,
    kIOSurfaceHeight, kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTexture, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};
use waos_webgpu_host::wasi::webgpu::webgpu::{
    GpuTextureDimension, GpuTextureFormat, GpuTextureUsage,
};
use waos_webgpu_host::{Device, GpuObject, TextureAttributes};

use crate::BufferKind;
use crate::bindings::wasdk::media::video_frames::{
    PlaneContent, PlaneFormat, PlanePixels, VideoPlane,
};
use crate::bindings::wasdk::platform::hardware_buffers::{
    AlphaType, HardwareBufferUsage, ImageError, ImageErrorKind, PixelBuffer, PixelFormat,
};
use crate::gpu::wgpu_types::error::{ErrorType, WebGpuError};
use crate::gpu::{GpuInstance, wgpu_core, wgpu_types};
use crate::hardware_buffers::Released;
use crate::os::{FinishHandOver, HandOver};
use crate::shell::{HardwareBuffer, VideoPicture};

/// The format a hardware buffer is allocated in, and so the only one a canvas can be configured
/// with: WebGPU's preferred canvas format on this platform.
pub(crate) const FORMAT: wgpu_types::TextureFormat = wgpu_types::TextureFormat::Bgra8Unorm;

/// The `OSType` for 32-bit BGRA, the layout Metal's `BGRA8Unorm` reads.
const BGRA_OS_TYPE: i32 = 0x4247_5241;

/// The `OSType`s of 8-bit 4:2:0 in two planes, brightness and then colour at half the size, in
/// video's range (`420v`) and the whole range (`420f`): what VideoToolbox decodes 8-bit video
/// into.
const TWO_PLANE_8_BIT_OS_TYPES: [u32; 2] = [0x3432_3076, 0x3432_3066];

/// One plane of a surface, as Metal, wgpu and a program each name its samples.
#[derive(Clone, Copy)]
struct SurfacePlane {
    index: usize,
    format: wgpu_types::TextureFormat,
    metal_format: MTLPixelFormat,
    gpu_format: GpuTextureFormat,
    plane_format: PlaneFormat,
    content: PlaneContent,
    bit_depth: u8,
}

/// The one plane of a BGRA surface: the whole picture.
const BGRA_PLANE: SurfacePlane = SurfacePlane {
    index: 0,
    format: FORMAT,
    metal_format: MTLPixelFormat::BGRA8Unorm,
    gpu_format: GpuTextureFormat::Bgra8unorm,
    plane_format: PlaneFormat::Bgra8,
    content: PlaneContent::Rgba,
    bit_depth: 8,
};

/// The two planes of 8-bit 4:2:0: brightness, then colour's two differences in two channels.
const TWO_PLANES_8_BIT: [SurfacePlane; 2] = [
    SurfacePlane {
        index: 0,
        format: wgpu_types::TextureFormat::R8Unorm,
        metal_format: MTLPixelFormat::R8Unorm,
        gpu_format: GpuTextureFormat::R8unorm,
        plane_format: PlaneFormat::R8,
        content: PlaneContent::Luma,
        bit_depth: 8,
    },
    SurfacePlane {
        index: 1,
        format: wgpu_types::TextureFormat::Rg8Unorm,
        metal_format: MTLPixelFormat::RG8Unorm,
        gpu_format: GpuTextureFormat::Rg8unorm,
        plane_format: PlaneFormat::Rg8,
        content: PlaneContent::Chroma,
        bit_depth: 8,
    },
];

/// The planes of a surface in `pixel_format`, as the engine makes textures of them; none for a
/// layout it makes none of. Ten-bit video is one: its samples are 16 bits, which WebGPU keeps
/// behind a feature the interface does not pass on yet.
fn surface_planes(pixel_format: u32) -> Option<&'static [SurfacePlane]> {
    if pixel_format == BGRA_OS_TYPE as u32 {
        Some(&[BGRA_PLANE])
    } else if TWO_PLANE_8_BIT_OS_TYPES.contains(&pixel_format) {
        Some(&TWO_PLANES_8_BIT)
    } else {
        None
    }
}

/// The planes of `buffer`'s picture, as a texture is made of each; none for a layout the engine
/// makes no textures of.
pub(crate) fn planes(buffer: &HardwareBufferMemory) -> Option<Vec<VideoPlane>> {
    let planes = surface_planes(buffer.surface.pixel_format())?;
    Some(
        planes
            .iter()
            .map(|plane| {
                let [width, height] = buffer.plane_size(plane);
                VideoPlane {
                    width,
                    height,
                    format: plane.plane_format,
                    content: plane.content,
                    bit_depth: plane.bit_depth,
                }
            })
            .collect(),
    )
}

/// The samples of each of `buffer`'s planes copied into the program's memory, rows as they lie,
/// while its surface is locked for reading, as IOSurface asks before the CPU reads it;
/// `unsupported` for a layout the engine tells no planes of.
pub(crate) fn read_planes(buffer: &HardwareBufferMemory) -> Result<Vec<PlanePixels>, ImageError> {
    let surface = &*buffer.surface;
    let format = surface.pixel_format();
    let Some(planes) = surface_planes(format) else {
        return Err(ImageError {
            kind: ImageErrorKind::Unsupported,
            message: format!("a buffer in the pixel format {format:#x} has no planes to read"),
        });
    };
    let options = IOSurfaceLockOptions::ReadOnly;
    // SAFETY: a read-only lock with no seed, released below.
    if unsafe { surface.lock(options, ptr::null_mut()) } != 0 {
        return Err(ImageError {
            kind: ImageErrorKind::Other,
            message: "the system did not lock the buffer's pixels".to_owned(),
        });
    }
    let read = planes
        .iter()
        .map(|plane| locked_plane_pixels(surface, plane))
        .collect();
    // SAFETY: the lock taken above, released once every plane is copied.
    unsafe { surface.unlock(options, ptr::null_mut()) };
    Ok(read)
}

/// The samples of `plane` of `surface`, which is locked for reading, copied out: its rows as they
/// lie, each the plane's own row length, and the surface's where it has one plane.
fn locked_plane_pixels(surface: &IOSurfaceRef, plane: &SurfacePlane) -> PlanePixels {
    let (base, bytes_per_row, height) = if surface.plane_count() == 0 {
        (
            surface.base_address(),
            surface.bytes_per_row(),
            surface.height(),
        )
    } else {
        (
            surface.base_address_of_plane(plane.index),
            surface.bytes_per_row_of_plane(plane.index),
            surface.height_of_plane(plane.index),
        )
    };
    // SAFETY: while the surface is locked, the plane's base address is mapped for `height` rows
    // of `bytes_per_row`, which are copied before the caller unlocks it.
    let bytes = unsafe {
        let base = base.as_ptr().cast::<u8>().cast_const();
        std::slice::from_raw_parts(base, bytes_per_row * height).to_vec()
    };
    PlanePixels {
        row_bytes: bytes_per_row as u32,
        bytes,
    }
}

/// A texture over the plane at `index` of `buffer`'s memory on the program's `device` of `gpu`,
/// which the GPU samples and copies from, as the program's `gpu-texture` once connected to the
/// device; `release` is called once wgpu has freed it. A plane the buffer does not have is the
/// program's mistake, and a failure the system answers the engine's.
///
/// On a lost device the texture is invalid, as one made there is.
pub(crate) fn plane_texture(
    buffer: &HardwareBufferMemory,
    gpu: &GpuInstance,
    device: &Device,
    index: u32,
    usage: wgpu_types::TextureUsages,
    release: wgpu_hal::DropCallback,
) -> Result<(Arc<GpuObject<wgpu_core::id::TextureId>>, TextureAttributes)> {
    let format = buffer.surface.pixel_format();
    let Some(plane) = surface_planes(format).and_then(|planes| planes.get(index as usize)) else {
        bail!("a hardware buffer in the format {format:#x} has no plane {index}")
    };
    let surface_texture = SurfaceTexture {
        label: "waos program hardware buffer plane",
        usage,
        view_formats: &[],
        metal_usage: MTLTextureUsage::ShaderRead,
        arrives: wgpu_types::TextureUses::RESOURCE,
        plane: *plane,
    };
    let size = buffer.plane_size(plane);
    let texture = surface_texture.over(
        &buffer.surface,
        size,
        gpu,
        device.device_id(),
        Some(release),
    )?;
    let attributes = TextureAttributes {
        size: wgpu_types::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: GpuTextureDimension::D2,
        format: plane.gpu_format,
        usage: gpu_usage(usage),
        texture_binding_view_dimension: None,
    };
    Ok((Arc::new(GpuObject::new(texture, gpu)), attributes))
}

/// Bytes one pixel of that format occupies.
const BYTES_PER_PIXEL: usize = 4;

/// Whether a program allocates hardware buffers on this system.
pub(crate) const ALLOCATES: bool = true;

/// What a buffer the system hands over is for, a decoded image's or a video frame's: sampled by
/// the GPU, as the system made it, and shown by Core Animation and read by the CPU, as any surface
/// is.
fn system_given() -> HardwareBufferUsage {
    HardwareBufferUsage::GPU_SAMPLED
        | HardwareBufferUsage::COMPOSER_OVERLAY
        | HardwareBufferUsage::CPU_READ
}

/// The largest side of a hardware buffer a program allocates, in pixels: the largest 2D texture
/// Metal makes on every Mac and on Apple's GPUs since the A9.
pub(crate) const LARGEST_SIDE: u32 = 16384;

/// Whether a texture over a hardware buffer is handed to the compositor and back: not on Apple's
/// systems, whose compositor shares the surface with no transfer.
pub(crate) const HANDS_TEXTURES_OVER: bool = false;

/// One hardware buffer's memory, as the engine holds it: the `IOSurface`, which a program's device
/// draws into through a texture made over it for each frame, and which the shell is handed as the
/// [`HardwareBuffer`] it shows, and what it can be used for. Cloning it is a retain: whoever still
/// holds one keeps the pixels alive.
#[derive(Clone)]
pub(crate) struct HardwareBufferMemory {
    surface: CFRetained<IOSurfaceRef>,
    width: u32,
    height: u32,
    usage: HardwareBufferUsage,
}

// SAFETY: an `IOSurface` is a thread-safe Core Foundation object: its retain count is atomic, it
// is made to be passed between processes, and any thread may wrap it in a texture. Rust cannot
// see that through the generated binding, which is neither; Waft's embedder vouches for the
// same when it carries one between threads.
unsafe impl Send for HardwareBufferMemory {}
unsafe impl Sync for HardwareBufferMemory {}

impl HardwareBufferMemory {
    /// Allocates a BGRA8 surface of `width` by `height` physical pixels, for every use whatever
    /// `usage` asks: any surface can be sampled, drawn into, shown by Core Animation and read by
    /// the CPU.
    pub(crate) fn new(
        width: u32,
        height: u32,
        usage: HardwareBufferUsage,
        mode: BufferKind,
    ) -> Result<HardwareBufferMemory> {
        let _ = (usage, mode);
        let surface = allocate(width as usize, height as usize)?;
        Ok(HardwareBufferMemory {
            surface,
            width,
            height,
            usage: HardwareBufferUsage::all(),
        })
    }

    /// Why a buffer in `format` for `usage` is not made here; none where it is: Apple's systems
    /// allocate surfaces in BGRA, each for every use.
    pub(crate) fn unsupported(
        format: PixelFormat,
        usage: HardwareBufferUsage,
    ) -> Option<&'static str> {
        let _ = usage;
        (format != PixelFormat::Bgra8)
            .then_some("Apple's systems allocate hardware buffers in BGRA")
    }

    /// A program's buffer of `width` by `height` pixels in `format`, sides the program checked:
    /// `unsupported` for RGBA, which this system's buffers are not allocated in, and
    /// `out-of-memory` where the system makes no surface. Every surface serves every use, so the
    /// buffer answers every flag of its usage, whatever `usage` asks.
    pub(crate) fn allocate(
        width: u32,
        height: u32,
        format: PixelFormat,
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

    /// The memory of `surface`, a hardware buffer the system handed over: a decoded image's, or a
    /// video frame's, for what the system made it for.
    pub(crate) fn of(surface: HardwareBuffer) -> HardwareBufferMemory {
        let (width, height) = (surface.width() as u32, surface.height() as u32);
        HardwareBufferMemory {
            surface,
            width,
            height,
            usage: system_given(),
        }
    }

    /// The size of `plane` of the surface: the surface's own for its one plane, and what the
    /// surface says of each where it has several.
    fn plane_size(&self, plane: &SurfacePlane) -> [u32; 2] {
        if self.surface.plane_count() == 0 {
            return [self.width, self.height];
        }
        [
            self.surface.width_of_plane(plane.index) as u32,
            self.surface.height_of_plane(plane.index) as u32,
        ]
    }

    /// The hardware buffer as the shell shows it. A retain, not a copy.
    pub(crate) fn buffer(&self) -> HardwareBuffer {
        self.surface.clone()
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// What the buffer can be used for: every use for a surface the engine allocated, and for one
    /// the system handed over what the system made it for, sampling, with the compositor's and
    /// the CPU's uses every surface has.
    pub(crate) fn usage(&self) -> HardwareBufferUsage {
        self.usage
    }

    /// The bytes the buffer's rows take: what a program is counted for holding it.
    pub(crate) fn bytes(&self) -> u64 {
        (self.surface.bytes_per_row() * self.surface.height()) as u64
    }

    /// The buffer's pixels copied into the program's memory, rows as they lie, BGRA and
    /// premultiplied, while its surface is locked for reading, as IOSurface asks before the CPU
    /// reads it; `unsupported` for a surface in another format.
    pub(crate) fn read_pixels(&self) -> Result<PixelBuffer, ImageError> {
        let surface = &*self.surface;
        let format = surface.pixel_format();
        if format != BGRA_OS_TYPE as u32 {
            return Err(ImageError {
                kind: ImageErrorKind::Unsupported,
                message: format!("a buffer in the pixel format {format:#x}, not 32-bit BGRA"),
            });
        }
        // SAFETY: a read-only lock with no seed, released below; the base address is read only
        // between the two, as `height` rows of `bytes_per_row`, and copied before the unlock.
        let (bytes_per_row, bytes) = unsafe {
            let options = IOSurfaceLockOptions::ReadOnly;
            if surface.lock(options, ptr::null_mut()) != 0 {
                return Err(ImageError {
                    kind: ImageErrorKind::Other,
                    message: "the system did not lock the buffer's pixels".to_owned(),
                });
            }
            let bytes_per_row = surface.bytes_per_row();
            let base = surface.base_address().as_ptr().cast::<u8>().cast_const();
            let bytes = std::slice::from_raw_parts(base, bytes_per_row * surface.height()).to_vec();
            surface.unlock(options, ptr::null_mut());
            (bytes_per_row, bytes)
        };
        Ok(PixelBuffer {
            width: self.width,
            height: self.height,
            row_bytes: bytes_per_row as u32,
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
        let format = self.surface.pixel_format();
        if format != BGRA_OS_TYPE as u32 {
            bail!("a hardware buffer is BGRA on Apple's systems, not the format {format:#x}")
        }
        let mut metal_usage = MTLTextureUsage::ShaderRead;
        if usage.contains(wgpu_types::TextureUsages::RENDER_ATTACHMENT) {
            metal_usage |= MTLTextureUsage::RenderTarget;
        }
        let surface_texture = SurfaceTexture {
            label: "waos program hardware buffer",
            usage,
            view_formats: &[],
            metal_usage,
            arrives: arriving_state(usage),
            plane: BGRA_PLANE,
        };
        let size = [self.width, self.height];
        let texture =
            surface_texture.over(&self.surface, size, gpu, device.device_id(), Some(release))?;
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
        _queue: wgpu_core::id::QueueId,
    ) -> Result<wgpu_core::id::TextureId> {
        let format = self.surface.pixel_format();
        if format != BGRA_OS_TYPE as u32 {
            bail!("a picture is BGRA on Apple's systems, not the format {format:#x}")
        }
        let sampled = SurfaceTexture {
            label: "waos screenshot picture",
            usage: wgpu_types::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
            metal_usage: MTLTextureUsage::ShaderRead,
            arrives: wgpu_types::TextureUses::RESOURCE,
            plane: BGRA_PLANE,
        };
        sampled.over(&self.surface, [self.width, self.height], gpu, device, None)
    }

    /// The surface marked in use, as the compositor marks one it shows, until what is answered
    /// goes: whoever learns a buffer is free from `IOSurfaceIsInUse`, as a composition layer does
    /// before it gives one back to its producer, waits for it meanwhile.
    pub(crate) fn in_use(&self) -> InUse {
        self.surface.increment_use_count();
        InUse(self.clone())
    }

    /// `released`, a release the engine hands the shell with the buffer, as it is: the compositor
    /// learns of a screenshot's reading from the surface's use count (`in_use`), which holds back
    /// its release itself.
    pub(crate) fn after_reads(&self, released: Released) -> Released {
        released
    }

    /// A wgpu texture over the hardware buffer, on the program's `device` of `gpu`, for `usage`
    /// and viewable in `view_formats`: what the program configured its canvas for, within what the
    /// Metal texture serves. One per frame: the program lets go of it once the frame is presented,
    /// which destroys the wgpu texture, while the surface underneath outlives every one of them.
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
        let mut metal_usage = MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead;
        // Metal views a texture in another format only where its usage allows it.
        if view_formats.iter().any(|format| *format != FORMAT) {
            metal_usage |= MTLTextureUsage::PixelFormatView;
        }
        let drawn = SurfaceTexture {
            label: "waos hardware buffer",
            usage,
            view_formats,
            metal_usage,
            // It arrives holding the last frame's pixels, which nothing reads: the program draws
            // the whole canvas, so declaring it uninitialized licenses nothing it wanted.
            arrives: wgpu_types::TextureUses::UNINITIALIZED,
            plane: BGRA_PLANE,
        };
        drawn.over(&self.surface, [self.width, self.height], gpu, device, None)
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
}

/// Hands a frame the program drew into `texture`, over a hardware buffer, to the compositor's
/// side once its work is done: here only what the program wrote on `device`'s queue and no
/// submission carried is submitted, since Metal and Core Animation share the surface with no
/// transfer. Answers that submission, after every one that drew the frame, for the wait before
/// the hand-over; none where wgpu-core refused it, as on a lost device. GPU work, submitted only
/// where the program's freeze allows it.
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

/// Takes `texture`, a frame's over a hardware buffer, back from the compositor before the program
/// draws into it: nothing on Apple's systems, where the compositor holds no claim on the
/// surface's memory that the program's device must take back.
pub(crate) fn take_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    let _ = (gpu, device, texture, usage);
}

/// Takes `texture`, a program's over its own hardware buffer, back from the compositor once it is
/// done with the buffer: nothing on Apple's systems, as for a frame.
pub(crate) fn take_back_from_compositor(
    gpu: &GpuInstance,
    device: &Device,
    _memory: &HardwareBufferMemory,
    texture: wgpu_core::id::TextureId,
    usage: wgpu_types::TextureUsages,
) {
    let _ = (gpu, device, texture, usage);
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
        BufferKind::GpuMemory(None),
    )
    .expect("a surface for a test");
    let surface = &*memory.surface;
    // SAFETY: a lock for writing with no seed, released below; the base address is written only
    // between the two, `height` rows of `bytes_per_row`, of which each pixel's 4 bytes.
    unsafe {
        let options = IOSurfaceLockOptions::empty();
        assert_eq!(
            surface.lock(options, ptr::null_mut()),
            0,
            "the surface locks"
        );
        let bytes_per_row = surface.bytes_per_row();
        let base = surface.base_address().as_ptr().cast::<u8>();
        for row in 0..height {
            for column in 0..width {
                let at = base.add(row as usize * bytes_per_row + column as usize * BYTES_PER_PIXEL);
                ptr::copy_nonoverlapping(pixel(column, row).as_ptr(), at, BYTES_PER_PIXEL);
            }
        }
        surface.unlock(options, ptr::null_mut());
    }
    memory
}

/// A hardware buffer's surface marked in use, until this goes.
pub(crate) struct InUse(HardwareBufferMemory);

impl Drop for InUse {
    fn drop(&mut self) {
        self.0.surface.decrement_use_count();
    }
}

/// A video's frame as the engine holds it: the system's `CVPixelBuffer`, whose `IOSurface` is its
/// hardware buffer. Cloning it is a retain: whoever still holds one keeps the frame's slot in its
/// decoder's pool, which a CoreVideo pool hands out again once nothing holds the pixel buffer,
/// whatever still reads its surface (measured: the pool decides by the surface's use count, not its
/// retain count).
#[derive(Clone)]
pub(crate) struct FrameMemory {
    frame: VideoPicture,
}

// SAFETY: a pixel buffer is a Core Foundation object whose retain count is atomic, and the engine
// only retains and releases it, from whichever thread holds it; Waft's macOS host vouches for the
// same when wgpu releases a frame on its own thread. Rust cannot see that through the generated
// binding, which is neither.
unsafe impl Send for FrameMemory {}
unsafe impl Sync for FrameMemory {}

impl FrameMemory {
    pub(crate) fn new(frame: VideoPicture) -> FrameMemory {
        FrameMemory { frame }
    }

    /// The frame's memory: the `IOSurface` its pixel buffer is on, in 32-bit BGRA, as the readers
    /// ask of their decoders, or in a decoder's own layout, whose planes a program reads where
    /// the engine makes textures of them (`planes`). A failure is the shell's, which handed out
    /// a frame on no surface.
    pub(crate) fn hardware_buffer(&self) -> Result<HardwareBufferMemory> {
        let Some(surface) = CVPixelBufferGetIOSurface(Some(&self.frame)) else {
            bail!("a video's frame is on an IOSurface, and this one is not")
        };
        Ok(HardwareBufferMemory::of(surface))
    }
}

/// How a texture over an `IOSurface` is used: what wgpu is told, the formats besides its own it is
/// viewed in, what Metal is told, and the state it arrives in.
struct SurfaceTexture<'a> {
    label: &'static str,
    usage: wgpu_types::TextureUsages,
    view_formats: &'a [wgpu_types::TextureFormat],
    metal_usage: MTLTextureUsage,
    arrives: wgpu_types::TextureUses,
    /// The plane of the surface it is over.
    plane: SurfacePlane,
}

impl SurfaceTexture<'_> {
    /// A wgpu texture over its plane of `surface`, of that plane's `size`, on `device` of `gpu`,
    /// made through a Metal texture on the surface; `release`, where given, is called once wgpu has freed it.
    ///
    /// On a lost device wgpu-core answers an invalid texture, with the loss as its error: the
    /// texture is handed out as it is.
    fn over(
        &self,
        surface: &IOSurfaceRef,
        [width, height]: [u32; 2],
        gpu: &wgpu_core::global::Global,
        device: wgpu_core::id::DeviceId,
        release: Option<wgpu_hal::DropCallback>,
    ) -> Result<wgpu_core::id::TextureId> {
        // SAFETY: the hal device is only read, and is dropped before the texture it makes is
        // handed back to wgpu.
        let hal = unsafe { gpu.device_as_hal::<wgpu_core::api::Metal>(device) };
        let Some(hal) = hal else {
            bail!("a hardware buffer needs the program's device to be a Metal device")
        };
        let metal = self.metal_texture(hal.raw_device(), surface, [width, height]);
        drop(hal);
        // SAFETY: `metal` is a live 2D texture in the plane's format, of exactly this size, with
        // one mip level and one layer, made on `device`.
        let texture = unsafe {
            wgpu_hal::metal::Device::texture_from_raw(
                metal,
                self.plane.format,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu_hal::CopyExtent {
                    width,
                    height,
                    depth: 1,
                },
                release,
            )
        };
        let descriptor = wgpu_types::TextureDescriptor {
            label: Some(std::borrow::Cow::Borrowed(self.label)),
            size: wgpu_types::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu_types::TextureDimension::D2,
            format: self.plane.format,
            usage: self.usage,
            view_formats: self.view_formats.to_vec(),
        };
        // SAFETY: the texture was made on `device` and matches the descriptor, whose usage is
        // within what the Metal texture serves, and is in the state `arrives` says.
        let (texture, error) = unsafe {
            gpu.create_texture_from_hal(Box::new(texture), device, &descriptor, self.arrives, None)
        };
        match error {
            Some(error) if error.webgpu_error_type() != ErrorType::DeviceLost => {
                bail!("{error:#?}")
            }
            _ => Ok(texture),
        }
    }

    /// A Metal texture over its plane of `surface`, on `device`.
    fn metal_texture(
        &self,
        device: &ProtocolObject<dyn MTLDevice>,
        surface: &IOSurfaceRef,
        [width, height]: [u32; 2],
    ) -> Retained<ProtocolObject<dyn MTLTexture>> {
        let descriptor = MTLTextureDescriptor::new();
        descriptor.setTextureType(MTLTextureType::Type2D);
        descriptor.setPixelFormat(self.plane.metal_format);
        // SAFETY: both fit an `NSUInteger`, and the surface's plane is of this size.
        unsafe {
            descriptor.setWidth(width as usize);
            descriptor.setHeight(height as usize);
        }
        // Shared is the storage mode an IOSurface-backed texture must take: it is what makes
        // the texture a view of the surface rather than a private copy of it.
        descriptor.setStorageMode(MTLStorageMode::Shared);
        descriptor.setUsage(self.metal_usage);
        device
            .newTextureWithDescriptor_iosurface_plane(&descriptor, surface, self.plane.index)
            .expect("a Metal texture over an IOSurface of a size Metal allows")
    }
}

/// The state a program's texture of `usage` arrives in, which its usage allows; its pixels are
/// taken as written, whoever wrote them: its decoder, Image I/O, or the program's earlier work.
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
    let mut answered = GpuTextureUsage::empty();
    for (wgpu, webgpu) in [
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
    ] {
        if usage.contains(wgpu) {
            answered |= webgpu;
        }
    }
    answered
}

/// Allocates an `IOSurface` of `width` by `height` BGRA8 pixels.
fn allocate(width: usize, height: usize) -> Result<CFRetained<IOSurfaceRef>> {
    // IOSurface picks its own row stride; asking for the aligned one keeps Metal's view of
    // the rows and everyone else's identical.
    // SAFETY: the keys are Core Foundation constants the framework exports.
    let keys: [&CFString; 5] = unsafe {
        [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfaceBytesPerElement,
            kIOSurfaceBytesPerRow,
            kIOSurfacePixelFormat,
        ]
    };
    let bytes_per_row = IOSurfaceRef::align_property(keys[3], width * BYTES_PER_PIXEL);
    let numbers = [
        CFNumber::new_isize(width as isize),
        CFNumber::new_isize(height as isize),
        CFNumber::new_isize(BYTES_PER_PIXEL as isize),
        CFNumber::new_isize(bytes_per_row as isize),
        CFNumber::new_i32(BGRA_OS_TYPE),
    ];
    let values: [&CFNumber; 5] = [
        &numbers[0],
        &numbers[1],
        &numbers[2],
        &numbers[3],
        &numbers[4],
    ];
    let properties = CFDictionary::from_slices(&keys, &values);
    // SAFETY: every value is of the type its key names.
    unsafe { IOSurfaceRef::new(properties.as_opaque()) }
        .context("allocating an IOSurface for a view's hardware buffers")
}
