//! When an input event happened, as the program reads it: the system's own stamp, passed through
//! in nanoseconds, on the input events' clock (the WIT's `pointer-event.time`).
//!
//! The invariant: an event's time is its stamp and nothing else, never the moment it is delivered,
//! so events the system hands over together keep the spacing they happened at, which a program's
//! velocity tracker reads.

use std::time::Duration;

/// `stamp`, a host event's time stamp, as the program's event time.
pub(crate) fn event_time(stamp: Duration) -> u64 {
    u64::try_from(stamp.as_nanos()).unwrap_or(u64::MAX)
}
