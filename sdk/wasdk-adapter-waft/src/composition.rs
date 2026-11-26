//! Waft's compositor over the WIT's, where the system has one (`get-compositor`): a layer an app
//! makes, for a `CompositionLayerView` or a player's picture, is a `composition-layer` the program
//! holds, and a view drawing a frame sets its layers to each layer the frame's scene places, with
//! its placement in the WIT's shape (`placements`), before it presents the frame's picture, as a
//! player is given the layer its picture goes to (`players`). The engine's shell shows the layers
//! beneath the picture. A hardware buffer the app presents on a layer is the WIT's `present`, and
//! its `released` runs once the WIT's answer comes, in a task of the program's own.
//!
//! The invariant: only a layer this compositor made crosses the WIT, as the handle the program
//! holds; one the app got elsewhere is left out of a view's layers. Only a buffer the WIT handed
//! out is presented; one the app got elsewhere goes back unshown.

use std::any::Any;
use std::sync::Arc;

use waft_embedder::{
    AcceptedBuffers, AcceptedFormat, CompositionLayer, Compositor, GpuMemory, HardwareBuffer,
    HardwareBufferFormat, LayerPicture, Placement,
};
use wasdk::{views, webgpu};

use crate::hardware_buffers::wit_buffer;
use crate::placements::placement_of;

/// The system's compositor, as Waft's app reaches it.
pub(crate) struct WasdkCompositor(views::Compositor);

impl WasdkCompositor {
    /// The system's compositor, where it has one.
    pub(crate) fn get() -> Option<WasdkCompositor> {
        views::get_compositor().map(WasdkCompositor)
    }
}

impl Compositor for WasdkCompositor {
    fn make_layer(&self, opaque: bool) -> Arc<dyn CompositionLayer> {
        Arc::new(WasdkLayer(self.0.make_layer(opaque)))
    }

    /// The buffers the system's allocator makes, in WebGPU's preferred canvas format, as a web
    /// page draws in it: what the system shows with no conversion. A program names no GPU, nor
    /// shows shared memory.
    fn accepted_buffers(&self) -> AcceptedBuffers {
        let format = match webgpu::get_gpu().get_preferred_canvas_format() {
            webgpu::GpuTextureFormat::Rgba8unorm => HardwareBufferFormat::Rgba8,
            _ => HardwareBufferFormat::Bgra8,
        };
        AcceptedBuffers {
            gpu_memory: vec![GpuMemory {
                gpu: None,
                formats: vec![AcceptedFormat {
                    format,
                    modifiers: Vec::new(),
                }],
            }],
            shared_memory: false,
        }
    }
}

/// A layer [`WasdkCompositor`] made: the WIT's, which a view's layers name.
struct WasdkLayer(views::CompositionLayer);

impl CompositionLayer for WasdkLayer {
    /// The WIT's `present` of the buffer behind `buffer`, `released` running once it answers. The
    /// WIT's layer takes no children, a program placing its layers through its views', so a
    /// present's are left out, and said so; a buffer the WIT did not hand out goes back unshown.
    fn present(
        &self,
        buffer: Arc<dyn HardwareBuffer>,
        children: Vec<(Arc<dyn CompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        if !children.is_empty() {
            tracing::warn!("a present's children are left out: a program's layer takes none");
        }
        let Some(buffer) = wit_buffer(buffer.as_ref()) else {
            tracing::warn!("a buffer the system did not hand out was presented; it is not shown");
            released();
            return;
        };
        let back = self.0.present(&buffer);
        wasdk::spawn(async move {
            back.await;
            drop(buffer);
            released();
        });
    }

    /// Nothing: what a program's layer shows is the shell's to read, and the WIT gives a program
    /// no way to.
    fn current_picture(&self) -> Option<LayerPicture> {
        None
    }
}

/// The layers a frame places, as the view is set to hold them: each the WIT's layer behind one
/// this compositor made, with its placement, in the order given, the bottom-most first.
pub(crate) fn placed_layers(
    layers: &[(Arc<dyn CompositionLayer>, Placement)],
) -> Vec<views::PlacedLayer<'_>> {
    layers
        .iter()
        .filter_map(|(layer, placement)| {
            Some(views::PlacedLayer {
                layer: wit_layer(layer)?,
                placement: placement_of(placement),
            })
        })
        .collect()
}

/// The WIT's layer behind `layer`; none for a layer another compositor made.
pub(crate) fn wit_layer(layer: &Arc<dyn CompositionLayer>) -> Option<&views::CompositionLayer> {
    let any: &dyn Any = &**layer;
    any.downcast_ref::<WasdkLayer>().map(|layer| &layer.0)
}
