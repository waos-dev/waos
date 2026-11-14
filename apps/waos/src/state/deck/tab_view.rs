//! The view a tab gives its program as the program starts: a canvas's composition layer, made on
//! Waft's compositor, and the queue the program's calls on the view arrive on, handed to the
//! engine as a view with no window and answering no request; once the program runs, the same
//! view shown, as `waos-program-window`'s `ShownView`, in the deck's view.
//!
//! The invariant: what the program draws goes to the layer from the moment it starts, and the
//! calls it makes before its tab shows it wait in the queue, carried out in their order once it
//! does.

use std::rc::Rc;
use std::sync::Arc;

use futures::channel::mpsc::{self, UnboundedReceiver};
use waft::ui::{Compositor, ViewMetrics, ViewRef};
use waft::{App, Entity};
use waos::bindings::wasdk::platform::windows::WindowBackground;
use waos::{PlacedView, Presence, ProgramView};
use waos_program_window::{CanvasLayer, CanvasView, ShownView, ViewCalls, ViewLink, metrics_of};
use waos_window_messages::ViewCall;

/// The view a tab gives its program as it starts: the canvas's layer, made on Waft's compositor,
/// and the queue its calls arrive on.
pub(crate) struct TabView {
    canvas: Arc<CanvasLayer>,
    calls: UnboundedReceiver<ViewCall>,
    /// What the program was told the view measures and how it stands, as it started.
    metrics: ViewMetrics,
    presence: Presence,
}

impl TabView {
    /// The view, with its canvas's layer made on `compositor`, opaque as a phone's screen is; and
    /// the same view as the engine is handed it, in a page `metrics` measures, standing as
    /// `presence` says.
    pub(crate) fn make(
        compositor: &dyn Compositor,
        metrics: &ViewMetrics,
        presence: Presence,
    ) -> (TabView, PlacedView) {
        let canvas = Arc::new(CanvasLayer::make(compositor, WindowBackground::Solid));
        let (queue, calls) = mpsc::unbounded();
        let calls_sent = ViewCalls::new(move |call| {
            // Once the tab is gone nobody carries the calls out, and nobody needs to.
            let _ = queue.unbounded_send(call);
        });
        let placed = PlacedView {
            view: Arc::new(CanvasView::new(
                Arc::clone(&canvas) as _,
                calls_sent.clone(),
            )),
            text_input: Some(Arc::new(calls_sent)),
            window: None,
            metrics: metrics_of(metrics),
            screen: metrics.screen.map(|screen| screen.0),
            presence,
            request: None,
        };
        let tab_view = TabView {
            canvas,
            calls,
            metrics: *metrics,
            presence,
        };
        (tab_view, placed)
    }

    /// The view shown as the program's `view`, in `waft_view`, in a page `metrics` measures now,
    /// standing as `presence` says now, carrying out the calls the program made on it so far and
    /// from now on; the program hears what changed while it started.
    pub(crate) fn show(
        self,
        app: &mut App,
        view: ProgramView,
        waft_view: ViewRef,
        metrics: ViewMetrics,
        presence: Presence,
    ) -> Entity<ShownView> {
        let shown = ShownView::open(
            app,
            Rc::new(view) as Rc<dyn ViewLink>,
            self.canvas,
            waft_view,
            self.metrics,
            self.presence,
            self.calls,
        );
        shown.update(app, |shown, _| {
            shown.set_metrics(metrics);
            shown.set_presence(presence);
        });
        shown
    }
}
