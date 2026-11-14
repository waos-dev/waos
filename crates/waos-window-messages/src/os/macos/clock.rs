//! The system's uptime clock, in nanoseconds: the clock Rust's `Instant` reads on macOS, which
//! every process reads alike, so a moment one process names is the same moment in another.

/// Now on the system's uptime clock, in nanoseconds.
pub fn uptime_now() -> u64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a timespec that outlives the call, and the clock always exists.
    let read = unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut now) };
    assert_eq!(read, 0, "the uptime clock is always there");
    now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_goes_forward() {
        let before = uptime_now();
        assert!(uptime_now() >= before);
        assert!(before > 0);
    }
}
