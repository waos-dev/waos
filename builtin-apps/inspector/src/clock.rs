//! Times as the user's clock shows them, in the user's time zone as the system says it; in UTC
//! where it has none.

use wasdk::system;
use wasdk::system_clock::Instant;

/// A day, in seconds.
const DAY: i64 = 24 * 60 * 60;

/// Now, in seconds since the epoch.
pub(crate) fn now() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
}

/// The start of today, UTC, as the device keeps its days.
pub(crate) fn today() -> i64 {
    now() - now().rem_euclid(DAY)
}

/// `at`, in seconds since the epoch, as the user's clock showed it: `14:03:27`.
pub(crate) fn time_of_day(at: i64) -> String {
    let local = at + offset_at(at);
    let seconds = local.rem_euclid(DAY);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// The weekday's initial of the UTC day `ago` days before today: M, T, W…
pub(crate) fn weekday_initial(ago: usize) -> &'static str {
    const INITIALS: [&str; 7] = ["T", "F", "S", "S", "M", "T", "W"];
    let ago = i64::try_from(ago).unwrap_or(0);
    let days = (today() / DAY - ago).rem_euclid(7);
    // The epoch's day was a Thursday.
    INITIALS[usize::try_from(days).unwrap_or(0)]
}

/// How far the user's clock is from UTC at `at`, in seconds.
fn offset_at(at: i64) -> i64 {
    let when = Instant {
        seconds: at,
        nanoseconds: 0,
    };
    system::utc_offset(when).map_or(0, |offset| offset / 1_000_000_000)
}
