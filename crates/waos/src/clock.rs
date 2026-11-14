//! The clock a program's time is on, and everything the engine hands it is stamped on.

use std::time::{Duration, Instant};

/// Nanoseconds since the program started, which is what its `wasi:clocks/monotonic-clock`
/// reads too. Stamping a view's events on it lets the program compare them with the time it
/// reads itself.
///
/// One clock per program, copied to whoever stamps: a view stamps the frames it hands over with
/// the refresh the shell reports, and the engine stamps presented frames on its own.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    origin: Instant,
}

impl Clock {
    pub fn start() -> Clock {
        Clock {
            origin: Instant::now(),
        }
    }

    pub fn now(&self) -> u64 {
        self.at(Instant::now())
    }

    /// The moment `instant` on this clock; 0 for one before the program started.
    pub fn at(&self, instant: Instant) -> u64 {
        instant.saturating_duration_since(self.origin).as_nanos() as u64
    }

    /// The moment the program names as `nanoseconds` on this clock, as the system's.
    pub fn instant_of(&self, nanoseconds: u64) -> Instant {
        self.origin + Duration::from_nanos(nanoseconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_on_the_clock_is_the_systems_again() {
        let clock = Clock::start();
        let moment = Instant::now() + Duration::from_millis(16);
        assert_eq!(clock.instant_of(clock.at(moment)), moment);
    }
}
