//! Where a motion let go would come to rest, as iOS projects a release: where it is, plus how far
//! its velocity would carry it on under a scroll view's normal deceleration. Every release of the
//! deck's, the overview's, the strip's and a card's throw, decides from this, so the decision
//! follows the way the finger moved over the last moment before it lifted, together with where the
//! value is, and never the way the drag went as a whole.
//!
//! The invariant: a release decides from the projection, and its spring still starts from where the
//! value is, at the finger's speed; the projection says only where it heads.

/// `UIScrollView.DecelerationRate.normal`: the share of its velocity a scroll view keeps after
/// each millisecond of gliding.
const NORMAL_DECELERATION_RATE: f64 = 0.998;

/// Where a value at `position`, let go at `velocity` in its units a second, would come to rest,
/// as Apple's "Designing Fluid Interfaces" (WWDC 2018) projects a release.
pub(super) fn projected(position: f64, velocity: f64) -> f64 {
    let rate = NORMAL_DECELERATION_RATE;
    position + velocity / 1000.0 * rate / (1.0 - rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_let_go_at_rest_stays_where_it_is() {
        assert_eq!(projected(120.0, 0.0), 120.0);
    }

    #[test]
    fn a_value_let_go_moving_carries_on_about_half_a_second_of_its_speed() {
        assert!((projected(0.0, 1000.0) - 499.0).abs() < 1e-6);
        assert!((projected(100.0, -200.0) - 0.2).abs() < 1e-6);
    }
}
