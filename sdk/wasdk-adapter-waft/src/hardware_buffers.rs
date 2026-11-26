//! Waft's hardware buffers over `wasdk:platform/hardware-buffers`, one to one: the system's
//! allocator for the program's device, where it allocates buffers, is Waft's
//! `HardwareBufferAllocator`, an allocation the WIT's `allocate`, its format and usage carried
//! field for field, a format the WIT names none of refused as `ImageErrorKind::Unsupported`; and
//! every buffer the WIT hands the program, one it allocated, an image the hardware codecs decoded
//! or a video frame's, is Waft's `HardwareBuffer` over it, whose usage is its `usage`, whose
//! format is its `format`, whose pixels are its `read-pixels`, and which is imported as a texture on the
//! program's device through the WIT's `gpu-interop` (`gpu_interop`). A side of no pixels, which
//! the WIT traps a program for, fails here first, as `ImageErrorKind::Other`, as Waft's hosts
//! answer it.
//!
//! The invariant: only a buffer the WIT handed out goes back to it, by the handle the program
//! holds, to be encoded or shown; one the app got elsewhere is refused.

use std::any::Any;
use std::sync::Arc;

use waft_embedder::image_codecs::{ImageError, ImageErrorKind, ImageFuture, PixelBuffer};
use waft_embedder::{
    HardwareBuffer, HardwareBufferAllocator, HardwareBufferFormat, HardwareBufferUsage,
};
use wasdk::hardware_buffers as platform;

use crate::image_codecs::{framework_image_error, framework_pixels};

/// The system's hardware buffer allocator for one device.
pub(crate) struct WasdkHardwareBufferAllocator(platform::HardwareBufferAllocator);

impl WasdkHardwareBufferAllocator {
    /// The system's allocator of buffers drawn on `device`, where it allocates buffers.
    pub(crate) fn get(device: &wgpu::Device) -> Option<WasdkHardwareBufferAllocator> {
        wasdk_wgpu::hardware_buffer_allocator(device).map(WasdkHardwareBufferAllocator)
    }
}

impl HardwareBufferAllocator for WasdkHardwareBufferAllocator {
    fn allocate(
        &self,
        width: u32,
        height: u32,
        format: HardwareBufferFormat,
        usage: HardwareBufferUsage,
    ) -> Result<Arc<dyn HardwareBuffer>, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::new(
                ImageErrorKind::Other,
                format!("a buffer of {width} by {height} pixels, which holds none"),
            ));
        }
        let format = wit_buffer_format(format).ok_or_else(|| {
            ImageError::new(
                ImageErrorKind::Unsupported,
                format!("a buffer in {format:?}, which wasdk names no format for"),
            )
        })?;
        let allocated = self.0.allocate(width, height, format, wit_usage(usage));
        allocated
            .map(framework_buffer)
            .map_err(framework_image_error)
    }
}

/// The WIT's hardware buffer as Waft's: one the program allocated, a decoded image's, or a
/// frame's.
pub(crate) fn framework_buffer(buffer: platform::HardwareBuffer) -> Arc<dyn HardwareBuffer> {
    Arc::new(WasdkHardwareBuffer(Arc::new(buffer)))
}

/// The WIT's hardware buffer behind `buffer`, which the hardware codecs encode, a layer shows and
/// the GPU imports; none for a buffer this platform did not hand out.
pub(crate) fn wit_buffer(buffer: &dyn HardwareBuffer) -> Option<Arc<platform::HardwareBuffer>> {
    let any: &dyn Any = buffer;
    any.downcast_ref::<WasdkHardwareBuffer>()
        .map(|buffer| Arc::clone(&buffer.0))
}

/// A hardware buffer the WIT handed out; shared with a read of its pixels, an encoding or a
/// present under way.
struct WasdkHardwareBuffer(Arc<platform::HardwareBuffer>);

impl HardwareBuffer for WasdkHardwareBuffer {
    fn width(&self) -> u32 {
        self.0.width()
    }

    fn height(&self) -> u32 {
        self.0.height()
    }

    /// The WIT's `usage`: what the system says the buffer can be used for.
    fn usage(&self) -> HardwareBufferUsage {
        framework_usage(self.0.usage())
    }

    /// The WIT's `format`: how the system's buffer lays out its pixels.
    fn format(&self) -> HardwareBufferFormat {
        framework_buffer_format(self.0.format())
    }

    /// The WIT's `read-pixels`, which waits for the GPU work that drew into the buffer itself.
    fn read_pixels(&self) -> ImageFuture<PixelBuffer> {
        let buffer = Arc::clone(&self.0);
        Box::pin(async move {
            let read = buffer.read_pixels().await.map_err(framework_image_error)?;
            framework_pixels(read)
        })
    }
}

/// Waft's buffer format as the WIT names it; none for one the WIT has no name for.
fn wit_buffer_format(format: HardwareBufferFormat) -> Option<platform::HardwareBufferFormat> {
    match format {
        HardwareBufferFormat::Bgra8 => Some(platform::HardwareBufferFormat::Bgra8),
        HardwareBufferFormat::Rgba8 => Some(platform::HardwareBufferFormat::Rgba8),
        HardwareBufferFormat::Nv12 => Some(platform::HardwareBufferFormat::Nv12),
        HardwareBufferFormat::P010 => Some(platform::HardwareBufferFormat::P010),
        _ => None,
    }
}

fn framework_buffer_format(format: platform::HardwareBufferFormat) -> HardwareBufferFormat {
    match format {
        platform::HardwareBufferFormat::Bgra8 => HardwareBufferFormat::Bgra8,
        platform::HardwareBufferFormat::Rgba8 => HardwareBufferFormat::Rgba8,
        platform::HardwareBufferFormat::Nv12 => HardwareBufferFormat::Nv12,
        platform::HardwareBufferFormat::P010 => HardwareBufferFormat::P010,
    }
}

fn wit_usage(usage: HardwareBufferUsage) -> platform::HardwareBufferUsage {
    let mut wit = platform::HardwareBufferUsage::empty();
    if usage.gpu_sampled {
        wit |= platform::HardwareBufferUsage::GPU_SAMPLED;
    }
    if usage.gpu_color_output {
        wit |= platform::HardwareBufferUsage::GPU_COLOR_OUTPUT;
    }
    if usage.composer_overlay {
        wit |= platform::HardwareBufferUsage::COMPOSER_OVERLAY;
    }
    if usage.cpu_read {
        wit |= platform::HardwareBufferUsage::CPU_READ;
    }
    wit
}

fn framework_usage(usage: platform::HardwareBufferUsage) -> HardwareBufferUsage {
    HardwareBufferUsage {
        gpu_sampled: usage.contains(platform::HardwareBufferUsage::GPU_SAMPLED),
        gpu_color_output: usage.contains(platform::HardwareBufferUsage::GPU_COLOR_OUTPUT),
        composer_overlay: usage.contains(platform::HardwareBufferUsage::COMPOSER_OVERLAY),
        cpu_read: usage.contains(platform::HardwareBufferUsage::CPU_READ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_usage_crosses_flag_for_flag() {
        assert_eq!(
            wit_usage(HardwareBufferUsage::default()),
            platform::HardwareBufferUsage::empty()
        );
        assert_eq!(
            wit_usage(HardwareBufferUsage {
                gpu_sampled: true,
                gpu_color_output: true,
                composer_overlay: true,
                cpu_read: true,
            }),
            platform::HardwareBufferUsage::GPU_SAMPLED
                | platform::HardwareBufferUsage::GPU_COLOR_OUTPUT
                | platform::HardwareBufferUsage::COMPOSER_OVERLAY
                | platform::HardwareBufferUsage::CPU_READ
        );
        assert_eq!(
            wit_usage(HardwareBufferUsage {
                cpu_read: true,
                ..HardwareBufferUsage::default()
            }),
            platform::HardwareBufferUsage::CPU_READ
        );
    }

    #[test]
    fn a_buffer_format_crosses_and_comes_back_by_name() {
        for format in [
            HardwareBufferFormat::Bgra8,
            HardwareBufferFormat::Rgba8,
            HardwareBufferFormat::Nv12,
            HardwareBufferFormat::P010,
        ] {
            let wit = wit_buffer_format(format).expect("a format the WIT names");
            assert_eq!(framework_buffer_format(wit), format);
        }
    }

    #[test]
    fn a_usage_comes_back_flag_for_flag() {
        for usage in [
            HardwareBufferUsage::default(),
            HardwareBufferUsage::ALL,
            HardwareBufferUsage::GPU_SAMPLED,
            HardwareBufferUsage {
                cpu_read: true,
                ..HardwareBufferUsage::default()
            },
        ] {
            assert_eq!(framework_usage(wit_usage(usage)), usage);
        }
    }
}
