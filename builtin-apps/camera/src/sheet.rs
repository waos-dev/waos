//! A sheet: GPU memory of the app's own that a picture is drawn onto, which the system's
//! compositor shows and its codecs encode with no copy through the program's memory. It is a
//! hardware buffer the system allocates, with the texture imported of it on the painter's device.

use std::sync::Arc;

use waft::ui::image_codecs::{ImageErrorKind, PixelFormat};
use waft::ui::{HardwareBuffer, HardwareBufferUsage, Platform};

/// The formats a sheet is allocated in, the first the system allocates: Apple's systems allocate
/// only the first, Android only the second.
const FORMATS: [PixelFormat; 2] = [PixelFormat::Bgra8, PixelFormat::Rgba8];

pub(crate) struct Sheet {
    pub(crate) buffer: Arc<dyn HardwareBuffer>,
    /// `buffer` as the painter's device draws onto it.
    pub(crate) texture: wgpu::Texture,
}

impl Sheet {
    /// A sheet of `size` pixels to draw onto on `device`, for a composition layer to show; why
    /// not, where the system allocates none. It is `gpu_sampled` too: Android composites a layer
    /// on the GPU wherever it shows it as no overlay, as an emulator always does, and blacks out a
    /// buffer the GPU may not sample.
    pub(crate) fn to_show(
        platform: &dyn Platform,
        device: &wgpu::Device,
        size: [u32; 2],
    ) -> Result<Sheet, String> {
        let usage = HardwareBufferUsage {
            gpu_color_output: true,
            gpu_sampled: true,
            composer_overlay: true,
            ..HardwareBufferUsage::default()
        };
        Sheet::allocate(platform, device, size, usage)
    }

    /// A sheet of `size` pixels to draw onto on `device`, for the system's codecs to encode; why
    /// not, where the system allocates none.
    pub(crate) fn to_encode(
        platform: &dyn Platform,
        device: &wgpu::Device,
        size: [u32; 2],
    ) -> Result<Sheet, String> {
        let usage = HardwareBufferUsage {
            gpu_color_output: true,
            ..HardwareBufferUsage::default()
        };
        Sheet::allocate(platform, device, size, usage)
    }

    /// A buffer for `usage` in the first of [`FORMATS`] the system allocates, with its texture.
    fn allocate(
        platform: &dyn Platform,
        device: &wgpu::Device,
        [width, height]: [u32; 2],
        usage: HardwareBufferUsage,
    ) -> Result<Sheet, String> {
        let allocator = platform
            .gpu(None)
            .and_then(|gpu| gpu.hardware_buffer_allocator())
            .ok_or("this system allocates no hardware buffers")?;
        let mut refusal = String::new();
        for format in FORMATS {
            match allocator.allocate(width, height, format, usage) {
                Ok(buffer) => return Sheet::of(platform, device, buffer),
                Err(error) if error.kind == ImageErrorKind::Unsupported => {
                    refusal = error.to_string();
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        Err(refusal)
    }

    fn of(
        platform: &dyn Platform,
        device: &wgpu::Device,
        buffer: Arc<dyn HardwareBuffer>,
    ) -> Result<Sheet, String> {
        let texture = platform
            .import_texture(buffer.as_ref(), device)
            .ok_or("the app's device cannot draw onto a hardware buffer")?;
        Ok(Sheet { buffer, texture })
    }
}
