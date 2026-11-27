//! The canvases of the views a program shows in its app, as WaOS keeps them: each a composition
//! layer of WaOS's own, which the engine presents the program's hardware buffers to with its
//! layers beneath them, placed in a layer context of its own, whose id the app's window shows
//! (the canvas is placed in the context's root as a view's frame places a layer in its window). A canvas is made before its window is asked for, placed at each
//! size the window reports, at once where nothing waits, or with the program's picture at that
//! size under the window's fence where the window waits in its resize for it (`canvas_pictures`),
//! and let go once its window is gone. Each of those is a call on its way to the main thread
//! (`state::program_apps`), where Core Animation's contexts and Waft's placing are.
//!
//! The invariant: a canvas's calls reach the main thread in the order made, its making first and
//! its removal last.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use waft::ui::{Matrix4, Mutator, Placement, Rect};
use waos::ViewRequest;
use waos::bindings::wasdk::platform::views::Metrics;
use waos_window_messages::os::macos::SendRight;

use super::canvas_pictures::{CanvasPictures, ToShow};
use super::window_process::WindowProcess;

/// The id WaOS gives the canvas of each window a program asks for in its app, which the canvas's
/// calls carry.
pub(crate) type CanvasKey = u64;

/// One call on the canvases, on its way to the main thread.
pub(crate) enum CanvasCall {
    /// Make the canvas of the window `request` asks for, which `process` then asks its app for,
    /// with where its layers are.
    Make {
        key: CanvasKey,
        process: Arc<WindowProcess>,
        request: ViewRequest,
    },
    /// Place the canvas at its view's size, as `metrics` give it, under `fence` when the window
    /// sent one with its own change of size.
    Place {
        key: CanvasKey,
        metrics: Metrics,
        fence: Option<SendRight>,
    },
    /// Show `shown` on the canvas, in its turn after what went before it: a picture, or the
    /// canvas laid out at the size its window waits for with the picture of that size; then
    /// `pictures` hears it is shown.
    Show {
        key: CanvasKey,
        shown: ToShow,
        pictures: Arc<CanvasPictures>,
    },
    /// Its window is gone: let it go.
    Remove(CanvasKey),
}

/// The canvases, as a window process reaches them: every call goes to the main thread.
pub(crate) struct CanvasesOnRequest {
    calls: UnboundedSender<CanvasCall>,
    last_key: AtomicU64,
}

impl CanvasesOnRequest {
    /// The canvases, and where their calls arrive on the main thread.
    pub(crate) fn new() -> (CanvasesOnRequest, UnboundedReceiver<CanvasCall>) {
        let (calls, arriving) = mpsc::unbounded();
        let canvases = CanvasesOnRequest {
            calls,
            last_key: AtomicU64::default(),
        };
        (canvases, arriving)
    }

    /// Makes the canvas of the window `request` asks for, then has `process` ask its app for the
    /// window; answers the canvas's key. A request that cannot reach the main thread, as WaOS
    /// quits, fails at once.
    pub(crate) fn make(&self, process: Arc<WindowProcess>, request: ViewRequest) -> CanvasKey {
        let key = self.last_key.fetch_add(1, Ordering::Relaxed) + 1;
        let made = self.calls.unbounded_send(CanvasCall::Make {
            key,
            process,
            request,
        });
        if let Err(unsent) = made
            && let CanvasCall::Make {
                process, request, ..
            } = unsent.into_inner()
        {
            process.canvas_refused(request, "WaOS is quitting");
        }
        key
    }

    /// Places the canvas of `key` at the size `metrics` give, under `fence`. A call that cannot
    /// reach the main thread lets the fence go with it, so the window waits for nothing.
    pub(crate) fn place(&self, key: CanvasKey, metrics: Metrics, fence: Option<SendRight>) {
        let _ = self.calls.unbounded_send(CanvasCall::Place {
            key,
            metrics,
            fence,
        });
    }

    /// Shows `shown` on the canvas of `key`, in its turn, and tells `pictures` once it is shown. A
    /// call that cannot reach the main thread, as WaOS quits, shows its picture where it is, so the
    /// buffer goes back.
    pub(crate) fn show(&self, key: CanvasKey, shown: ToShow, pictures: Arc<CanvasPictures>) {
        let sent = self.calls.unbounded_send(CanvasCall::Show {
            key,
            shown,
            pictures,
        });
        if let Err(unsent) = sent
            && let CanvasCall::Show {
                shown, pictures, ..
            } = unsent.into_inner()
        {
            pictures.show_here(shown);
        }
    }

    /// Lets the canvas of `key` go.
    pub(crate) fn remove(&self, key: CanvasKey) {
        let _ = self.calls.unbounded_send(CanvasCall::Remove(key));
    }
}

/// Where a layer that fills its context goes in it, a view's canvas or a prompt's: its placement,
/// as a view's scene places a layer, the view in its logical pixels under the root's transform to
/// its physical ones (`Placement::mutators`), so what is drawn at the view's physical size shows
/// whole (`Placement::scale`); the root's size, in points; and how many pixels make a point.
pub(crate) struct CanvasPlace {
    pub(crate) placement: Placement,
    pub(crate) root_width: f64,
    pub(crate) root_height: f64,
    pub(crate) scale: f64,
}

impl CanvasPlace {
    /// For a program's view of `metrics`.
    pub(crate) fn of(metrics: &Metrics) -> CanvasPlace {
        let size = [f64::from(metrics.width), f64::from(metrics.height)];
        CanvasPlace::filling(size, metrics.device_pixel_ratio)
    }

    /// For a view `physical_size` pixels large, `scale` of which make a point.
    pub(crate) fn filling(physical_size: [f64; 2], scale: f64) -> CanvasPlace {
        let [width, height] = physical_size.map(|side| side / scale);
        let to_physical = Matrix4::scale(scale as f32, scale as f32);
        CanvasPlace {
            placement: Placement {
                rect: Rect::from_ltwh(0.0, 0.0, width, height),
                mutators: vec![Mutator::Transform(to_physical)],
            },
            root_width: width,
            root_height: height,
            scale,
        }
    }
}

#[cfg(test)]
mod tests {
    use waos::bindings::wasdk::platform::views::Insets;

    use super::*;

    /// A view 1600 by 1200 pixels at 2 to the point, in a window 800 by 600 points: the canvas
    /// fills it, and a buffer the program draws at the view's size shows whole, which Waft's host
    /// does where the placement's scale is the view's (`Placement::scale`).
    #[test]
    fn a_canvas_shows_what_is_drawn_at_its_views_physical_size_whole() {
        let none = Insets {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        };
        let place = CanvasPlace::of(&Metrics {
            width: 1600,
            height: 1200,
            device_pixel_ratio: 2.0,
            padding: none,
            view_padding: none,
            view_insets: none,
        });
        assert_eq!(
            place.placement.bounds(),
            Rect::from_ltwh(0.0, 0.0, 1600.0, 1200.0),
            "it covers the view's pixels"
        );
        assert_eq!(
            place.placement.rect,
            Rect::from_ltwh(0.0, 0.0, 800.0, 600.0),
            "in the view's logical pixels"
        );
        assert_eq!(place.placement.scale(), 2.0, "under the view's scale");
        assert_eq!((place.root_width, place.root_height), (800.0, 600.0));
        assert_eq!(place.scale, 2.0);
    }
}
