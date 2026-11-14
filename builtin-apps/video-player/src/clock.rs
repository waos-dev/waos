//! A time in media as a clock shows it, in the controls and the recent list.

use std::time::Duration;

/// `time` as `4:05`, or `1:02:03` from an hour on.
pub(crate) fn clock(time: Duration) -> String {
    let seconds = time.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_shows_minutes_and_seconds_and_hours_from_an_hour_on() {
        assert_eq!(clock(Duration::from_millis(4_999)), "0:04");
        assert_eq!(clock(Duration::from_secs(245)), "4:05");
        assert_eq!(clock(Duration::from_secs(3723)), "1:02:03");
    }
}
