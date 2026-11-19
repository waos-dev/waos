//! A program's composition layers on Waft's compositor: the shell's side of the WIT's
//! `composition-layer`, [`WaftCompositor`], whose every layer is one of Waft's that the engine
//! holds for the program, which shows a hardware buffer the program presents to it as Waft's buffer
//! over the platform's own (`waft_buffer`), answers what it shows now as the platform's own again
//! (`picture_of`), a raw buffer as its surface, its `AHardwareBuffer`, its dmabuf or its shared
//! texture and a lent frame as its pixel buffer, and is Waft's again with [`waft_layer`], for what
//! the program hands it to, such as a player; and the layers a program's view holds as it presents, as
//! the children Waft's layer of the program's canvas shows beneath the program's picture
//! (`canvas_layer`), each placed as Waft places a layer (`placements`).
//!
//! The engine hands back the layers it was handed, so every layer a present names was made here,
//! and is Waft's again by a downcast, as Waft's hosts get theirs back. The invariant: a layer
//! that was not made here is left out of the children, and said so, rather than shown wrong.

use std::any::Any;
use std::sync::Arc;

use waft::ui::{self, CompositionLayer, Compositor, Placement};
#[cfg(any(target_vendor = "apple", target_os = "android"))]
use waos::VideoFrame;
use waos::bindings::wasdk::platform::views;
use waos::{HardwareBuffer, LayerPicture, ShellCompositionLayer, ShellCompositor};

use super::placements::placement_of;

/// Waft's compositor, making a program's layers.
pub struct WaftCompositor(Arc<dyn Compositor>);

impl WaftCompositor {
    pub fn new(compositor: Arc<dyn Compositor>) -> WaftCompositor {
        WaftCompositor(compositor)
    }
}

impl ShellCompositor for WaftCompositor {
    fn make_layer(&self, opaque: bool) -> Arc<dyn ShellCompositionLayer> {
        Arc::new(ProgramLayer(self.0.make_layer(opaque)))
    }
}

/// A layer a program made: Waft's, as the engine holds it.
struct ProgramLayer(Arc<dyn CompositionLayer>);

impl ShellCompositionLayer for ProgramLayer {
    /// Waft's layer shows the buffer, as Waft's own, with no children: a program places its
    /// layers through its view's.
    fn present(&self, buffer: HardwareBuffer, released: Box<dyn FnOnce() + Send + Sync>) {
        let Some(waft) = waft_buffer(buffer) else {
            tracing::warn!("an offscreen hardware buffer cannot be presented to Waft's compositor");
            released();
            return;
        };
        self.0.present(waft, Vec::new(), released);
    }

    fn current_picture(&self) -> Option<LayerPicture> {
        picture_of(self.0.current_picture()?)
    }
}

/// `layers`, as a program's present named them, as the children of its canvas's layer: Waft's
/// layer of each, placed as Waft places one, in the order named, the bottom-most first.
pub(crate) fn children_of(
    layers: Vec<(Arc<dyn ShellCompositionLayer>, views::Placement)>,
) -> Vec<(Arc<dyn CompositionLayer>, Placement)> {
    layers
        .into_iter()
        .filter_map(|(layer, placement)| Some((named_layer(layer)?, placement_of(placement))))
        .collect()
}

/// Waft's layer behind `layer`; none for a layer no [`WaftCompositor`] made.
pub fn waft_layer(layer: Arc<dyn ShellCompositionLayer>) -> Option<Arc<dyn CompositionLayer>> {
    let layer: Arc<dyn Any + Send + Sync> = layer;
    let layer = layer.downcast::<ProgramLayer>().ok()?;
    Some(Arc::clone(&layer.0))
}

/// The engine's hardware `buffer`, the platform's own, as Waft's buffer over it, which a Waft
/// layer shows: on Apple's systems Waft's surface buffer over the `IOSurface`, for every use, as
/// any surface is.
#[cfg(target_vendor = "apple")]
pub(crate) fn waft_buffer(buffer: HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
    Some(Arc::new(
        waft_embedder_apple::hardware_buffers::SurfaceBuffer::new(buffer),
    ))
}

/// The engine's hardware `buffer`, the platform's own, as Waft's buffer over it, which a Waft
/// layer shows: on Android Waft's buffer over the `AHardwareBuffer`, a reference more on it, for
/// what it was allocated for.
#[cfg(target_os = "android")]
pub(crate) fn waft_buffer(buffer: HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
    Some(Arc::new(waft_embedder_android::AndroidHardwareBuffer::new(
        buffer.buffer().clone(),
    )))
}

/// The engine's hardware `buffer`, the platform's own, as Waft's buffer over it, which a Waft
/// layer shows: on Linux Waft's dmabuf buffer over a duplicate of its fd, for every use, as linear
/// memory is, made once for the dmabuf, so its `wl_buffer` is made once too. Returns `None` for
/// offscreen buffers (no dmabuf fd).
#[cfg(target_os = "linux")]
pub(crate) fn waft_buffer(buffer: HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
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

/// The engine's hardware `buffer`, the platform's own, as Waft's buffer over it, which a Waft
/// layer shows: on Windows Waft's shared texture over a duplicate of its NT handle, for every GPU
/// use, made once for the texture, so Waft opens it on its device once too.
#[cfg(windows)]
pub(crate) fn waft_buffer(buffer: HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
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

/// A platform whose hardware buffers are not made yet: there is no buffer to show.
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    windows
)))]
pub(crate) fn waft_buffer(buffer: HardwareBuffer) -> Option<Arc<dyn ui::HardwareBuffer>> {
    match buffer {}
}

/// What Waft's layer shows, `picture`, as the engine is handed it, case for case: a buffer as the
/// `IOSurface` it is, and a frame as its pixel buffer, retained, which keeps its slot in its pool;
/// none, said so, for a buffer or a frame Waft's Apple host did not hand out.
#[cfg(target_vendor = "apple")]
fn picture_of(picture: ui::LayerPicture) -> Option<LayerPicture> {
    use objc2_core_foundation::Type;

    match picture {
        ui::LayerPicture::Buffer(buffer) => {
            let buffer: &dyn Any = &*buffer;
            let Some(buffer) =
                buffer.downcast_ref::<waft_embedder_apple::hardware_buffers::SurfaceBuffer>()
            else {
                tracing::warn!("a layer showed a buffer Waft's host did not hand out; left out");
                return None;
            };
            Some(LayerPicture::Buffer(buffer.surface().retain()))
        }
        ui::LayerPicture::Frame(frame) => {
            let frame: &dyn Any = &*frame;
            let Some(frame) = frame.downcast_ref::<waft_embedder_apple::media::CoreVideoFrame>()
            else {
                tracing::warn!("a layer showed a frame Waft's host did not hand out; left out");
                return None;
            };
            Some(LayerPicture::Frame(VideoFrame {
                picture: frame.pixel_buffer().retain(),
                rotation: ui::VideoFrame::rotation(frame),
                flip: ui::VideoFrame::flip(frame),
            }))
        }
    }
}

/// What Waft's layer shows, `picture`, as the engine is handed it: a buffer, or a camera's frame,
/// as the `AHardwareBuffer` it is, a reference more on it, the frame with how it is shown; none,
/// said so, for one Waft's Android host did not hand out.
#[cfg(target_os = "android")]
fn picture_of(picture: ui::LayerPicture) -> Option<LayerPicture> {
    match picture {
        ui::LayerPicture::Buffer(buffer) => {
            let buffer: &dyn Any = &*buffer;
            let Some(buffer) =
                buffer.downcast_ref::<waft_embedder_android::AndroidHardwareBuffer>()
            else {
                tracing::warn!("a layer showed a buffer Waft's host did not hand out; left out");
                return None;
            };
            Some(LayerPicture::Buffer(HardwareBuffer::from(
                buffer.buffer().clone(),
            )))
        }
        ui::LayerPicture::Frame(frame) => {
            let frame: &dyn Any = &*frame;
            let Some(frame) = frame.downcast_ref::<waft_embedder_android::AndroidVideoFrame>()
            else {
                tracing::warn!("a layer showed a frame Waft's host did not hand out; left out");
                return None;
            };
            Some(LayerPicture::Frame(VideoFrame {
                picture: HardwareBuffer::from(frame.buffer().buffer().clone()),
                rotation: ui::VideoFrame::rotation(frame),
                flip: ui::VideoFrame::flip(frame),
            }))
        }
    }
}

/// What Waft's layer shows, `picture`, as the engine is handed it: a buffer as the dmabuf it is,
/// over a duplicate of its fd; none, said so, for a buffer Waft's Linux host did not hand out, and
/// for a frame, which it hands out none of.
#[cfg(target_os = "linux")]
fn picture_of(picture: ui::LayerPicture) -> Option<LayerPicture> {
    let ui::LayerPicture::Buffer(buffer) = picture else {
        tracing::warn!("a layer showed a frame, which Linux's host hands out none of; left out");
        return None;
    };
    let buffer: &dyn Any = &*buffer;
    let Some(buffer) = buffer.downcast_ref::<waft_embedder_winit::DmabufBuffer>() else {
        tracing::warn!("a layer showed a buffer Waft's host did not hand out; left out");
        return None;
    };
    let fd = buffer.fd().try_clone().ok()?;
    let size = [
        ui::HardwareBuffer::width(buffer),
        ui::HardwareBuffer::height(buffer),
    ];
    Some(LayerPicture::Buffer(HardwareBuffer::new(
        fd,
        size,
        buffer.stride(),
        buffer.offset(),
        buffer.modifier(),
    )))
}

/// What Waft's layer shows, `picture`, as the engine is handed it: a buffer as the shared texture
/// it is, over a duplicate of its NT handle; none, said so, for a buffer Waft's Windows host did
/// not hand out, and for a frame, which it hands out none of.
#[cfg(windows)]
fn picture_of(picture: ui::LayerPicture) -> Option<LayerPicture> {
    let ui::LayerPicture::Buffer(buffer) = picture else {
        tracing::warn!("a layer showed a frame, which Windows' host hands out none of; left out");
        return None;
    };
    let buffer: &dyn Any = &*buffer;
    let Some(buffer) = buffer.downcast_ref::<waft_embedder_winit::SharedTexture>() else {
        tracing::warn!("a layer showed a buffer Waft's host did not hand out; left out");
        return None;
    };
    let handle = buffer.handle().try_clone().ok()?;
    let size = [
        ui::HardwareBuffer::width(buffer),
        ui::HardwareBuffer::height(buffer),
    ];
    Some(LayerPicture::Buffer(HardwareBuffer::new(handle, size)))
}

/// A platform whose hardware buffers are not made yet: the engine takes none of a layer's.
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    windows
)))]
fn picture_of(_picture: ui::LayerPicture) -> Option<LayerPicture> {
    None
}

/// Waft's layer behind `layer`, which a present named; none, said so, for a layer no
/// [`WaftCompositor`] made.
fn named_layer(layer: Arc<dyn ShellCompositionLayer>) -> Option<Arc<dyn CompositionLayer>> {
    let waft = waft_layer(layer);
    if waft.is_none() {
        tracing::warn!("a present named a layer Waft's compositor did not make; it is left out");
    }
    waft
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use waft::ui::{HardwareBuffer, Rect};
    use waos::bindings::wasdk::platform::windows;

    use super::*;

    /// A layer of Waft's that shows nothing.
    struct Shown;

    impl CompositionLayer for Shown {
        fn present(
            &self,
            _buffer: Arc<dyn HardwareBuffer>,
            _children: Vec<(Arc<dyn CompositionLayer>, Placement)>,
            _released: Box<dyn FnOnce() + Send + Sync>,
        ) {
        }

        fn current_picture(&self) -> Option<ui::LayerPicture> {
            None
        }
    }

    /// Waft's compositor, keeping each layer it made and whether it was asked opaque.
    #[derive(Default)]
    struct Made(Mutex<Vec<(Arc<dyn CompositionLayer>, bool)>>);

    impl Compositor for Made {
        fn make_layer(&self, opaque: bool) -> Arc<dyn CompositionLayer> {
            let layer: Arc<dyn CompositionLayer> = Arc::new(Shown);
            self.0.lock().unwrap().push((Arc::clone(&layer), opaque));
            layer
        }
    }

    /// A layer no compositor of Waft's made.
    struct Foreign;

    impl ShellCompositionLayer for Foreign {
        fn present(
            &self,
            _buffer: waos::HardwareBuffer,
            released: Box<dyn FnOnce() + Send + Sync>,
        ) {
            released();
        }

        fn current_picture(&self) -> Option<LayerPicture> {
            None
        }
    }

    fn at(x: f64) -> views::Placement {
        views::Placement {
            rect: windows::Rect {
                origin: windows::Point { x, y: 0.0 },
                size: windows::Size {
                    width: 10.0,
                    height: 10.0,
                },
            },
            mutators: Vec::new(),
        }
    }

    #[test]
    fn a_programs_layers_are_wafts_own_in_the_order_named() {
        let made = Arc::new(Made::default());
        let compositor = WaftCompositor::new(Arc::clone(&made) as Arc<dyn Compositor>);
        let video = compositor.make_layer(true);
        let badge = compositor.make_layer(false);
        let children = children_of(vec![(badge, at(1.0)), (video, at(2.0))]);
        let made = made.0.lock().unwrap();
        assert_eq!(
            made.iter().map(|(_, opaque)| *opaque).collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(children.len(), 2);
        assert!(Arc::ptr_eq(&children[0].0, &made[1].0), "the badge first");
        assert!(
            Arc::ptr_eq(&children[1].0, &made[0].0),
            "the video beneath it"
        );
        assert_eq!(children[0].1.rect, Rect::from_ltwh(1.0, 0.0, 10.0, 10.0));
        assert_eq!(children[1].1.rect, Rect::from_ltwh(2.0, 0.0, 10.0, 10.0));
    }

    #[test]
    fn a_programs_layer_is_wafts_again_and_one_made_elsewhere_is_none() {
        let made = Arc::new(Made::default());
        let compositor = WaftCompositor::new(Arc::clone(&made) as Arc<dyn Compositor>);
        let layer = waft_layer(compositor.make_layer(false)).unwrap();
        assert!(Arc::ptr_eq(&layer, &made.0.lock().unwrap()[0].0));
        assert!(waft_layer(Arc::new(Foreign)).is_none());
    }

    #[test]
    fn a_layer_made_elsewhere_is_left_out() {
        let compositor = WaftCompositor::new(Arc::new(Made::default()));
        let ours = compositor.make_layer(false);
        let children = children_of(vec![(Arc::new(Foreign), at(0.0)), (ours, at(1.0))]);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].1.rect, Rect::from_ltwh(1.0, 0.0, 10.0, 10.0));
    }
}
