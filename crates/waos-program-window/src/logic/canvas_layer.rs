//! The composition layer a program's canvas shows: what the program's hardware buffers are
//! presented to, from any thread, in the process the program runs in, with the program's own layers
//! as its children, beneath its picture (`composition`). Whoever shows the canvas places it: the
//! frames of the view's window, where the window is in the same process, or WaOS, in the context a
//! program's window process hosts. The canvas's layer is as opaque as the window's background: a
//! composition layer blends its own buffer while a present gives it children, so the program's
//! layers show through the room its picture makes for them either way.
//!
//! The invariant: once the window is gone ([`CanvasLayer::window_gone`]), a hardware buffer
//! presented goes straight back, since nothing will show it, and the layers presented with it are
//! not shown.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use waft::ui::{CompositionLayer, Compositor};
use waos::bindings::wasdk::platform::views::Placement;
use waos::bindings::wasdk::platform::windows::WindowBackground;
use waos::{HardwareBuffer, ShellCompositionLayer};

use super::canvas_view::Canvas;
use super::composition::{children_of, waft_buffer};

pub struct CanvasLayer {
    layer: Arc<dyn CompositionLayer>,
    /// Whether the view's window is gone.
    gone: AtomicBool,
}

impl CanvasLayer {
    /// The layer of a canvas whose window has `background`, made by `compositor`: opaque where the
    /// background is solid, since nothing behind the window shows through it then.
    pub fn make(compositor: &dyn Compositor, background: WindowBackground) -> CanvasLayer {
        let opaque = background == WindowBackground::Solid;
        CanvasLayer {
            layer: compositor.make_layer(opaque),
            gone: AtomicBool::new(false),
        }
    }

    /// The layer, for whoever shows the canvas to place.
    pub fn layer(&self) -> Arc<dyn CompositionLayer> {
        Arc::clone(&self.layer)
    }

    /// The view's window is gone: what is presented from now on goes nowhere.
    pub fn window_gone(&self) {
        self.gone.store(true, Ordering::SeqCst);
    }
}

impl Canvas for CanvasLayer {
    /// Shows `buffer` from the compositor's next frame on, with `layers`, the program's, beneath
    /// it, and calls `released` once the compositor is done with it; at once, while the window is
    /// gone.
    fn present(
        &self,
        buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        if self.gone.load(Ordering::SeqCst) {
            released();
            return;
        }
        let children = children_of(layers);
        self.layer.present(waft_buffer(buffer), children, released);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine presents from its frames' threads.
    #[test]
    fn the_layer_crosses_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<CanvasLayer>();
    }
}
