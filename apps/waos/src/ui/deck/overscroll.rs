//! Dragging past an end the way an iOS scroll view resists it: Waft's `BouncingScrollPhysics`,
//! asked directly with a snapshot of the metrics instead of through a `Scrollable`.
//!
//! The deck moves several things a scroll view would (the pages, the overview's grid, a card being
//! thrown) without being one, because one stack of pages has to turn into a grid of cards without
//! being re-parented. Asking the physics directly keeps the resistance and the glide identical to
//! a Cupertino scroll view's.

use waft::{AxisDirection, BouncingScrollPhysics, FixedScrollMetrics, ScrollPhysics};
use waft_physics::Simulation;

/// The extent a value may move in freely, and the size of the view it moves in, which sets how
/// quickly the resistance past either end grows.
#[derive(Clone, Copy, Debug)]
pub(super) struct DragRange {
    pub(super) min: f64,
    pub(super) max: f64,
    pub(super) viewport: f64,
}

/// Where `pixels` goes when a finger moves by `finger_delta`, in a scroll view's convention: the
/// value moves against the finger, so a finger moving left or up increases it.
pub(super) fn scrolled_by_finger(pixels: f64, finger_delta: f64, range: DragRange) -> f64 {
    if finger_delta == 0.0 {
        return pixels;
    }
    let metrics = metrics(pixels, range);
    let applied = BouncingScrollPhysics::new().apply_physics_to_user_offset(&metrics, finger_delta);
    pixels - applied
}

/// Where `value` goes when a finger moves by `finger_delta`, for a value that moves with the
/// finger, as a card's sideways offset does.
pub(super) fn followed_by_finger(value: f64, finger_delta: f64, range: DragRange) -> f64 {
    let mirrored = DragRange {
        min: -range.max,
        max: -range.min,
        viewport: range.viewport,
    };
    -scrolled_by_finger(-value, finger_delta, mirrored)
}

/// What a scroll view does after the finger lifts at `finger_velocity`: glide with friction
/// inside the range, and spring back to an end it was dragged past. `None` when it is at rest
/// inside the range.
pub(super) fn released(
    pixels: f64,
    finger_velocity: f64,
    range: DragRange,
) -> Option<Box<dyn Simulation>> {
    let metrics = metrics(pixels, range);
    BouncingScrollPhysics::new().create_ballistic_simulation(&metrics, -finger_velocity)
}

fn metrics(pixels: f64, range: DragRange) -> FixedScrollMetrics {
    FixedScrollMetrics::new(
        Some(range.min),
        Some(range.max),
        Some(pixels),
        Some(range.viewport),
        AxisDirection::Down,
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGE: DragRange = DragRange {
        min: 0.0,
        max: 1000.0,
        viewport: 400.0,
    };

    #[test]
    fn inside_the_range_the_value_moves_one_to_one() {
        assert_eq!(scrolled_by_finger(500.0, -30.0, RANGE), 530.0);
        assert_eq!(followed_by_finger(500.0, -30.0, RANGE), 470.0);
    }

    #[test]
    fn past_an_end_the_value_moves_less_than_the_finger() {
        let past_start = scrolled_by_finger(-20.0, 30.0, RANGE);
        assert!(past_start < -20.0 && past_start > -50.0);
        let past_end = followed_by_finger(1020.0, 30.0, RANGE);
        assert!(past_end > 1020.0 && past_end < 1050.0);
    }

    #[test]
    fn a_value_let_go_past_an_end_springs_back_to_it() {
        let simulation = released(-40.0, 0.0, RANGE).expect("out of range");
        assert!(simulation.x(0.0) < 0.0);
        assert!((simulation.x(3.0) - 0.0).abs() < 0.5);
    }
}
