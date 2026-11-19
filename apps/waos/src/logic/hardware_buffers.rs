//! A program's hardware buffer between the engine and Waft: the engine's, the platform's own
//! buffer, an `IOSurface` on Apple's systems, an `AHardwareBuffer` on Android, a dmabuf on Linux
//! and a texture shared by NT handle on Windows, and Waft's buffer over it, which Waft's codecs
//! encode; and back, a buffer Waft's hardware codecs decoded, as the engine's.

use std::sync::Arc;

use waft::ui;
use waos::HardwareBuffer;

/// Waft's `buffer` as the engine's: the `IOSurface` behind Waft's surface buffer, which holds it
/// too; none, said so, for a buffer of another kind.
#[cfg(target_vendor = "apple")]
pub(crate) fn engine_buffer(buffer: &dyn ui::HardwareBuffer) -> Option<HardwareBuffer> {
    let buffer: &dyn std::any::Any = buffer;
    let Some(buffer) =
        buffer.downcast_ref::<waft_embedder_apple::hardware_buffers::SurfaceBuffer>()
    else {
        tracing::warn!("Waft answered a hardware buffer that is not an IOSurface; it is dropped");
        return None;
    };
    Some(HardwareBuffer::from(buffer.surface()))
}

/// Waft's `buffer` as the engine's: the `AHardwareBuffer` behind Waft's Android buffer, a
/// reference more on it; none, said so, for a buffer of another kind.
#[cfg(target_os = "android")]
pub(crate) fn engine_buffer(buffer: &dyn ui::HardwareBuffer) -> Option<HardwareBuffer> {
    let buffer: &dyn std::any::Any = buffer;
    let Some(buffer) = buffer.downcast_ref::<waft_embedder_android::AndroidHardwareBuffer>() else {
        tracing::warn!(
            "Waft answered a hardware buffer that is not an AHardwareBuffer; it is dropped"
        );
        return None;
    };
    Some(HardwareBuffer::from(buffer.buffer().clone()))
}

/// Waft's `buffer` as the engine's: the dmabuf behind Waft's Linux buffer, over a duplicate of its
/// fd; none, said so, for a buffer of another kind.
#[cfg(target_os = "linux")]
pub(crate) fn engine_buffer(buffer: &dyn ui::HardwareBuffer) -> Option<HardwareBuffer> {
    let buffer: &dyn std::any::Any = buffer;
    let Some(buffer) = buffer.downcast_ref::<waft_embedder_winit::DmabufBuffer>() else {
        tracing::warn!("Waft answered a hardware buffer that is not a dmabuf; it is dropped");
        return None;
    };
    let fd = buffer.fd().try_clone().ok()?;
    let size = [
        ui::HardwareBuffer::width(buffer),
        ui::HardwareBuffer::height(buffer),
    ];
    Some(HardwareBuffer::new(
        fd,
        size,
        buffer.stride(),
        buffer.offset(),
        buffer.modifier(),
    ))
}

/// Waft's `buffer` as the engine's: the texture behind Waft's shared texture, over a duplicate of
/// its NT handle; none, said so, for a buffer of another kind.
#[cfg(windows)]
pub(crate) fn engine_buffer(buffer: &dyn ui::HardwareBuffer) -> Option<HardwareBuffer> {
    let buffer: &dyn std::any::Any = buffer;
    let Some(buffer) = buffer.downcast_ref::<waft_embedder_winit::SharedTexture>() else {
        tracing::warn!(
            "Waft answered a hardware buffer that is not a shared texture; it is dropped"
        );
        return None;
    };
    let handle = buffer.handle().try_clone().ok()?;
    let size = [
        ui::HardwareBuffer::width(buffer),
        ui::HardwareBuffer::height(buffer),
    ];
    Some(HardwareBuffer::new(handle, size))
}

/// A platform whose buffers the engine does not take yet: none.
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    windows
)))]
pub(crate) fn engine_buffer(_buffer: &dyn ui::HardwareBuffer) -> Option<HardwareBuffer> {
    None
}

/// The engine's `buffer` as Waft's: Waft's surface buffer over the `IOSurface`, which holds it, for
/// every use, as any surface is, and which Waft only reads.
#[cfg(target_vendor = "apple")]
pub(crate) fn waft_buffer(buffer: &HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
    Some(Arc::new(
        waft_embedder_apple::hardware_buffers::SurfaceBuffer::new(buffer.clone()),
    ))
}

/// The engine's `buffer` as Waft's: Waft's Android buffer over the `AHardwareBuffer`, a reference
/// more on it, for what it was allocated for, and which Waft only reads.
#[cfg(target_os = "android")]
pub(crate) fn waft_buffer(buffer: &HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
    Some(Arc::new(waft_embedder_android::AndroidHardwareBuffer::new(
        buffer.buffer().clone(),
    )))
}

/// The engine's `buffer` as Waft's: Waft's dmabuf buffer over a duplicate of its fd, for every
/// use, as linear memory is, made once for the dmabuf, so its `wl_buffer` is made once too.
/// Returns `None` for offscreen buffers (no dmabuf fd).
#[cfg(target_os = "linux")]
pub(crate) fn waft_buffer(buffer: &HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
    let (fd, stride, offset, modifier) = (
        buffer.fd()?,
        buffer.stride()?,
        buffer.offset()?,
        buffer.modifier()?,
    );
    let made = buffer.shells(|| {
        let fd = fd.try_clone().expect("duplicating a dmabuf's fd");
        waft_embedder_winit::DmabufBuffer::new(
            fd,
            buffer.size(),
            stride,
            offset,
            modifier,
            ui::HardwareBufferUsage::ALL,
        )
    });
    made.map(|made| made as Arc<dyn ui::HardwareBuffer>)
}

/// The engine's `buffer` as Waft's: Waft's shared texture over a duplicate of its NT handle, for
/// every GPU use, made once for the texture, so Waft opens it on its device once too.
#[cfg(windows)]
pub(crate) fn waft_buffer(buffer: &HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
    let made = buffer.shells(|| {
        let handle = buffer
            .handle()
            .try_clone()
            .expect("duplicating a shared texture's handle");
        let usage = ui::HardwareBufferUsage {
            cpu_read: false,
            ..ui::HardwareBufferUsage::ALL
        };
        waft_embedder_winit::SharedTexture::new(handle, buffer.size(), usage)
    });
    made.map(|made| made as Arc<dyn ui::HardwareBuffer>)
}

/// A platform whose buffers the engine does not take yet: there is no buffer.
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    windows
)))]
pub(crate) fn waft_buffer(buffer: &HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
    match *buffer {}
}
