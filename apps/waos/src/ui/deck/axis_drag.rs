//! A `GestureDetector`'s drag along one axis, reported as what the deck's gestures read: a finger
//! came down, started to drag, moved this far along the axis and is here in the view now, lifted
//! at this speed along the axis, or never came to drag at all.
//!
//! A detector is given every one of its drags' callbacks at every build, whatever the deck is
//! doing: the set of gestures a detector recognizes never changes while a finger is down, since a
//! recognizer taken away mid-gesture is disposed without a word, and what holds a value for it
//! would hold it for ever. What a gesture does at a given moment is decided inside its callbacks.

use std::rc::Rc;
use std::time::Duration;

use waft::{App, GestureDetector, Listener, Offset};
use waft_gestures::{
    DragDownDetails, DragEndDetails, DragStartDetails, DragUpdateDetails, GestureDragDownCallback,
    GestureDragEndCallback, GestureDragStartCallback, GestureDragUpdateCallback,
};

/// What one axis's drag does.
#[derive(Clone)]
pub(super) struct AxisDrag {
    /// A finger came down, and may come to drag.
    pub(super) on_down: FingerChanged,
    /// The drag won over the other gestures the finger could have been, and starts, the finger
    /// being at this point of the view.
    pub(super) on_start: FingerStarted,
    /// The finger moved.
    pub(super) on_update: FingerMoved,
    /// The finger lifted at this velocity along the axis, in points a second.
    pub(super) on_end: FingerLifted,
    /// The finger lifted before it dragged, or was another gesture's.
    pub(super) on_cancel: FingerChanged,
}

/// One move of a dragging finger.
#[derive(Clone, Copy, Debug)]
pub(super) struct FingerMove {
    /// How far it moved along the drag's axis, in points.
    pub(super) along: f64,
    /// Where in the view it is now, across the axis too.
    pub(super) at: Offset,
    /// When, on the clock the system stamps its events with, where it says.
    pub(super) time: Option<Duration>,
}

/// A finger came down, or was lost.
pub(super) type FingerChanged = Rc<dyn Fn(&mut App)>;
/// A finger started to drag, at this point of the view.
pub(super) type FingerStarted = Rc<dyn Fn(&mut App, Offset)>;
pub(super) type FingerMoved = Rc<dyn Fn(&mut App, FingerMove)>;
/// A finger lifted, at so many points a second along the axis.
pub(super) type FingerLifted = Rc<dyn Fn(&mut App, f64)>;

/// A `GestureDetector` that reports drags along an axis as an [`AxisDrag`].
pub(super) trait AxisDragCallbacks {
    fn horizontal_axis_drag(self, drag: AxisDrag) -> Self;
    fn vertical_axis_drag(self, drag: AxisDrag) -> Self;
}

impl AxisDragCallbacks for GestureDetector {
    fn horizontal_axis_drag(self, drag: AxisDrag) -> GestureDetector {
        let callbacks = DetectorCallbacks::of(drag);
        self.on_horizontal_drag_down(callbacks.down)
            .on_horizontal_drag_start(callbacks.start)
            .on_horizontal_drag_update(callbacks.update)
            .on_horizontal_drag_end(callbacks.end)
            .on_horizontal_drag_cancel(callbacks.cancel)
    }

    fn vertical_axis_drag(self, drag: AxisDrag) -> GestureDetector {
        let callbacks = DetectorCallbacks::of(drag);
        self.on_vertical_drag_down(callbacks.down)
            .on_vertical_drag_start(callbacks.start)
            .on_vertical_drag_update(callbacks.update)
            .on_vertical_drag_end(callbacks.end)
            .on_vertical_drag_cancel(callbacks.cancel)
    }
}

struct DetectorCallbacks {
    down: GestureDragDownCallback,
    start: GestureDragStartCallback,
    update: GestureDragUpdateCallback,
    end: GestureDragEndCallback,
    cancel: Listener,
}

impl DetectorCallbacks {
    fn of(drag: AxisDrag) -> DetectorCallbacks {
        let AxisDrag {
            on_down,
            on_start,
            on_update,
            on_end,
            on_cancel,
        } = drag;
        DetectorCallbacks {
            down: Rc::new(move |app: &mut App, _: DragDownDetails| on_down(app)),
            start: Rc::new(move |app: &mut App, details: DragStartDetails| {
                on_start(app, details.global_position);
            }),
            update: Rc::new(move |app: &mut App, details: DragUpdateDetails| {
                let moved = FingerMove {
                    along: details.primary_delta.unwrap_or(0.0),
                    at: details.global_position,
                    time: details.source_time_stamp,
                };
                on_update(app, moved);
            }),
            end: Rc::new(move |app: &mut App, details: DragEndDetails| {
                on_end(app, details.primary_velocity.unwrap_or(0.0));
            }),
            cancel: Listener::new(move |app| on_cancel(app)),
        }
    }
}
