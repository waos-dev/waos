//! The springs every motion of the deck settles with, named for what they move, and the one way a
//! motion is set moving: from wherever its value is, at the velocity the finger gave it or the one
//! it already had.
//!
//! Every animated value is an unbounded `AnimationController` driven by a simulation, never by a
//! duration and a curve. That is what lets a finger catch it anywhere: stopping the controller
//! leaves the value where it was, and the next spring starts from there at the next velocity. A
//! duration-and-curve animation restarted halfway jumps in speed.
//!
//! The invariant: a spring lands exactly on its target, so what waits for a motion to be over
//! compares the value with the target.

use std::time::Duration;

use waft::{AnimationController, App, Handle};
use waft_physics::{SpringDescription, SpringSimulation, Tolerance};
use waft_scheduler::TickerFuture;

/// How near its target a motion is, on screen, when it is over, in points.
const RESTING_DISTANCE: f64 = 0.25;
/// How slow a motion is, on screen, when it is over, in points a second.
const RESTING_SPEED: f64 = 15.0;

/// A page settling after a swipe on the bar: quick and without bounce, as a paging scroll view
/// settles.
pub(super) fn page_swipe() -> SpringDescription {
    SpringDescription::with_duration_and_bounce(Duration::from_millis(380), 0.0)
}

/// A page shrinking into its card in the overview. The small bounce is the overshoot a flick
/// into the overview carries.
pub(super) fn overview_opening() -> SpringDescription {
    SpringDescription::with_duration_and_bounce(Duration::from_millis(460), 0.12)
}

/// A card growing back into its page: as quick, without the bounce, which a page that fills its
/// box has no room to show.
pub(super) fn overview_closing() -> SpringDescription {
    SpringDescription::with_duration_and_bounce(Duration::from_millis(460), 0.0)
}

/// A card let go short of closing, returning to its place.
pub(super) fn card_return() -> SpringDescription {
    SpringDescription::with_duration_and_bounce(Duration::from_millis(380), 0.18)
}

/// A card thrown off the screen to close its tab.
pub(super) fn card_throw() -> SpringDescription {
    SpringDescription::with_duration_and_bounce(Duration::from_millis(320), 0.0)
}

/// Cards moving into the places a closed card left.
pub(super) fn card_reflow() -> SpringDescription {
    SpringDescription::with_duration_and_bounce(Duration::from_millis(420), 0.1)
}

/// Settles `controller` on `target` with `spring`, starting from its current value at `velocity`,
/// in the controller's units a second.
///
/// `points_per_unit` says how far one unit of the value moves something on screen, so the spring
/// is over when the motion can no longer be seen rather than at a precision that means nothing
/// for this value. The value lands exactly on `target`.
pub(super) fn settle(
    app: &mut App,
    controller: Handle<AnimationController>,
    target: f64,
    velocity: f64,
    spring: SpringDescription,
    points_per_unit: f64,
) -> TickerFuture {
    let tolerance = Tolerance {
        distance: RESTING_DISTANCE / points_per_unit,
        velocity: RESTING_SPEED / points_per_unit,
        ..Tolerance::DEFAULT_TOLERANCE
    };
    let start = controller.value(app);
    let simulation =
        SpringSimulation::with_options(spring, start, target, velocity, true, tolerance);
    controller.animate_with(app, Box::new(simulation))
}
