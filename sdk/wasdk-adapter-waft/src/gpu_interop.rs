//! Waft's `Platform::import_texture` over `wasdk:platform/gpu-interop`: a buffer the WIT handed the
//! program, one it allocated, a decoded image's or a frame's, imported as a texture on the
//! program's device through the SDK (`wasdk_wgpu::import_texture`), which the WIT always gives,
//! for what the buffer's usage names, holding the buffer, and a frame's buffer its frame, until the
//! GPU is done with it.
//!
//! The invariant: only a buffer the WIT handed out goes back to it, by the handle the program
//! holds; one the app got elsewhere is imported as no texture.

use waft_embedder::HardwareBuffer;

use crate::hardware_buffers::wit_buffer;

/// `buffer` imported as a texture on `device`, the program's; none for a buffer this platform did
/// not hand out.
pub(crate) fn import_texture(
    buffer: &dyn HardwareBuffer,
    device: &wgpu::Device,
) -> Option<wgpu::Texture> {
    let buffer = wit_buffer(buffer)?;
    Some(wasdk_wgpu::import_texture(device, &buffer))
}
