//! `wasdk:platform/gpu-interop`: a hardware buffer the program holds imported as a texture on its
//! own WebGPU device, over the buffer's memory with no copy (`os`), as each system's GPU API
//! imports such memory, decision 36 of `docs/proposals/2026-11-02-program-media.md`. What the
//! texture is for follows from what the buffer was made for ([`texture_usages`]): the buffer keeps
//! only its own usage, named after its users, and its interface names no GPU API. The interface is
//! served to a program by `host`.
//!
//! The invariant: a texture imported of a buffer holds the buffer, and a video frame's buffer its
//! frame, until wgpu frees the texture, once the GPU is done with it (`hardware_buffers`).

pub(crate) mod host;

use crate::bindings::wasdk::platform::hardware_buffers::HardwareBufferUsage;
use crate::gpu::wgpu_types;

/// What a texture of a buffer made for `usage` is for: sampled where the GPU samples the buffer,
/// drawn into and copied into where it draws into it, and copied from always. The compositor's
/// and the CPU's usages are not the GPU's, and add nothing.
fn texture_usages(usage: HardwareBufferUsage) -> wgpu_types::TextureUsages {
    let mut usages = wgpu_types::TextureUsages::COPY_SRC;
    if usage.contains(HardwareBufferUsage::GPU_SAMPLED) {
        usages |= wgpu_types::TextureUsages::TEXTURE_BINDING;
    }
    if usage.contains(HardwareBufferUsage::GPU_COLOR_OUTPUT) {
        usages |=
            wgpu_types::TextureUsages::RENDER_ATTACHMENT | wgpu_types::TextureUsages::COPY_DST;
    }
    usages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_drawn_into_is_copied_into_and_every_buffer_copied_from() {
        assert_eq!(
            texture_usages(HardwareBufferUsage::empty()),
            wgpu_types::TextureUsages::COPY_SRC
        );
        assert_eq!(
            texture_usages(HardwareBufferUsage::GPU_SAMPLED),
            wgpu_types::TextureUsages::TEXTURE_BINDING | wgpu_types::TextureUsages::COPY_SRC
        );
        assert_eq!(
            texture_usages(HardwareBufferUsage::GPU_COLOR_OUTPUT),
            wgpu_types::TextureUsages::COPY_SRC
                | wgpu_types::TextureUsages::COPY_DST
                | wgpu_types::TextureUsages::RENDER_ATTACHMENT
        );
    }

    #[test]
    fn the_compositor_and_the_cpu_add_no_texture_usage() {
        assert_eq!(
            texture_usages(HardwareBufferUsage::COMPOSER_OVERLAY | HardwareBufferUsage::CPU_READ),
            wgpu_types::TextureUsages::COPY_SRC
        );
    }
}
