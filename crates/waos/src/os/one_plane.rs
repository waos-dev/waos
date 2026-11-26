//! A buffer's planes on the systems where every buffer the engine takes is one plane of red,
//! green and blue: all but Apple's, where a decoder's picture is taken in two. The names are the
//! Apple module's, so the rest of the crate asks either the same.

use std::sync::Arc;

use anyhow::{Result, bail};
use waos_gpu_host::{Device, GpuObject, TextureDescriptor};

use super::{FORMAT, HardwareBufferMemory};
use crate::bindings::wasdk::media::video_frames::{
    PlaneContent, PlaneFormat, PlanePixels, VideoPlane,
};
use crate::bindings::wasdk::platform::hardware_buffers::ImageError;
use crate::gpu::{GpuInstance, wgpu_core, wgpu_types};

/// The one plane of `buffer`'s picture: the whole of it, in the system's buffer format.
pub(crate) fn planes(buffer: &HardwareBufferMemory) -> Option<Vec<VideoPlane>> {
    let format = match FORMAT {
        wgpu_types::TextureFormat::Rgba8Unorm => PlaneFormat::Rgba8,
        _ => PlaneFormat::Bgra8,
    };
    Some(vec![VideoPlane {
        width: buffer.width(),
        height: buffer.height(),
        format,
        content: PlaneContent::Rgba,
        bit_depth: 8,
    }])
}

/// The samples of `buffer`'s one plane copied into the program's memory: its pixels, as the
/// buffer reads them.
pub(crate) fn read_planes(buffer: &HardwareBufferMemory) -> Result<Vec<PlanePixels>, ImageError> {
    let pixels = buffer.read_pixels()?;
    Ok(vec![PlanePixels {
        row_bytes: pixels.row_bytes,
        bytes: pixels.bytes,
    }])
}

/// A texture over the plane at `index` of `buffer`'s memory: the buffer's own texture for its
/// one plane. Another plane is the program's mistake.
pub(crate) fn plane_texture(
    buffer: &HardwareBufferMemory,
    gpu: &GpuInstance,
    device: &Device,
    index: u32,
    usage: wgpu_types::TextureUsages,
    release: Box<dyn FnOnce() + Send + Sync>,
) -> Result<(Arc<GpuObject<wgpu_core::id::TextureId>>, TextureDescriptor)> {
    if index != 0 {
        bail!("a hardware buffer of one plane has no plane {index}")
    }
    buffer.program_texture(gpu, device, usage, release)
}
