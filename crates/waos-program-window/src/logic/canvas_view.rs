//! The shell's side of a program's view, as the engine holds it: the view's canvas, which the
//! engine presents the program's hardware buffers to from any thread, with the program's own
//! layers; the cursor and the focus, which only the canvas can change, on their way to it, and
//! whether the program handles Back and the style of the system's bars over the view, on their
//! way to whatever shows the view: to
//! the main thread where the window is in the same process, to the window process where the
//! program's windows are an app of their own. The view's frames are paced by its screen's
//! refreshes, which are the engine's to ask for, not the view's.
//!
//! The invariant: frames never pass through the main thread's widgets; a hardware buffer goes to
//! the view's [`Canvas`].

use std::sync::Arc;

use waos::bindings::wasdk::platform::views::{
    Cursor, OverlayStyle, Placement, ViewFocusDirection, ViewFocusState,
};
use waos::{HardwareBuffer, ShellCompositionLayer, ShellView};
use waos_window_messages::ViewCall;

use super::view_calls::ViewCalls;

/// What a view's pictures are presented to, from the engine's frame threads: the composition
/// layer its canvas shows (`CanvasLayer`), or what shows a picture with its window's new size, as
/// WaOS does for a view in a program's app on macOS.
pub trait Canvas: Send + Sync {
    /// Shows `buffer` with `layers`, the program's, beneath it, and calls `released` once the
    /// compositor is done with it, as `ShellView::present` says.
    fn present(
        &self,
        buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    );
}

pub struct CanvasView {
    canvas: Arc<dyn Canvas>,
    calls: ViewCalls,
}

impl CanvasView {
    /// The view whose hardware buffers go to `canvas`, and whose cursor, focus, Back and bars'
    /// style go as `calls`.
    pub fn new(canvas: Arc<dyn Canvas>, calls: ViewCalls) -> CanvasView {
        CanvasView { canvas, calls }
    }
}

impl ShellView for CanvasView {
    fn present(
        &self,
        buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        self.canvas.present(buffer, layers, released);
    }

    fn set_cursor(&self, cursor: Cursor) {
        self.calls.send(ViewCall::SetCursor(cursor));
    }

    fn request_focus_change(&self, state: ViewFocusState, direction: ViewFocusDirection) {
        self.calls
            .send(ViewCall::RequestFocusChange(state, direction));
    }

    fn set_handles_back(&self, handles: bool) {
        self.calls.send(ViewCall::SetHandlesBack(handles));
    }

    fn set_overlay_style(&self, style: OverlayStyle) {
        self.calls.send(ViewCall::SetOverlayStyle(style));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine calls the view from the program's thread and its frames' own.
    #[test]
    fn the_view_crosses_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<CanvasView>();
    }
}
