//! The program's clock, as `wasi:clocks/monotonic-clock` reads it.

use wasmtime_wasi::HostMonotonicClock;

use crate::clock::Clock;

/// A program's [`Clock`] as WASI's monotonic clock, so that the time a program reads is the
/// same nanoseconds the engine stamps its frames and events with.
pub(super) struct MonotonicClock(Clock);

impl MonotonicClock {
    pub(super) fn new(clock: Clock) -> MonotonicClock {
        MonotonicClock(clock)
    }
}

impl HostMonotonicClock for MonotonicClock {
    fn resolution(&self) -> u64 {
        1
    }

    fn now(&self) -> u64 {
        self.0.now()
    }
}
