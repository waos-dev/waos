//! A moment on the program's monotonic clock, as the WIT names it (`instant`, in nanoseconds) and
//! as Waft names it (`std::time::Instant`): the same moment either way, each turned into the other
//! by its distance from now, read on both clocks together, since std's `Instant` is opaque.
//!
//! The invariant: a moment turned into the other kind and back lands where it was, give or take
//! the time between two readings of the clocks.

use std::time::{Duration, Instant};

use wasdk::monotonic_clock;
use wasdk::views;

/// The WIT's moment `at`, as Waft's.
pub(crate) fn framework_instant(at: views::Instant) -> Instant {
    framework_instant_from(at, monotonic_clock::now(), Instant::now())
}

/// Waft's moment `at`, as the WIT's.
pub(crate) fn wit_instant(at: Instant) -> views::Instant {
    wit_instant_from(at, Instant::now(), monotonic_clock::now())
}

/// `at` as Waft's moment, `wit_now` and `now` being the same moment on each clock.
fn framework_instant_from(at: views::Instant, wit_now: views::Instant, now: Instant) -> Instant {
    if at >= wit_now {
        now + Duration::from_nanos(at - wit_now)
    } else {
        let before = Duration::from_nanos(wit_now - at);
        now.checked_sub(before).unwrap_or(now)
    }
}

/// `at` as the WIT's moment, `now` and `wit_now` being the same moment on each clock; the
/// nearest the WIT's nanoseconds hold, for a moment beyond them.
fn wit_instant_from(at: Instant, now: Instant, wit_now: views::Instant) -> views::Instant {
    match at.checked_duration_since(now) {
        Some(ahead) => wit_now.saturating_add(nanoseconds(ahead)),
        None => wit_now.saturating_sub(nanoseconds(now - at)),
    }
}

fn nanoseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_ahead_or_behind_keeps_its_distance_from_now_either_way() {
        let (now, wit_now) = (Instant::now(), 5_000_000_000);
        let ahead = framework_instant_from(wit_now + 16_000_000, wit_now, now);
        assert_eq!(ahead - now, Duration::from_millis(16));
        assert_eq!(wit_instant_from(ahead, now, wit_now), wit_now + 16_000_000);
        let behind = now - Duration::from_millis(3);
        assert_eq!(wit_instant_from(behind, now, wit_now), wit_now - 3_000_000);
        assert_eq!(
            framework_instant_from(wit_now - 3_000_000, wit_now, now),
            behind
        );
    }
}
