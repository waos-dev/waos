//! When an item was published, as a list and the reading page say it: how long ago within a week,
//! and the date before that, on the user's clock: the moment moved by how far the user's time zone
//! is from UTC at it, which the caller gives (`users_offset`, the system's).

use wasdk::system;
use wasdk::system_clock::Instant;

/// How far local time is from UTC at a moment in seconds since 1970, in seconds.
pub(crate) type UtcOffset = fn(u64) -> i64;

/// How far the user's time zone is from UTC at `at`, as the system says; none, as UTC, where the
/// system names no time zone.
pub(crate) fn users_offset(at: u64) -> i64 {
    let instant = Instant {
        seconds: i64::try_from(at).unwrap_or(i64::MAX),
        nanoseconds: 0,
    };
    system::utc_offset(instant).map_or(0, |nanoseconds| nanoseconds / 1_000_000_000)
}

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;
const WEEK: u64 = 7 * DAY;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `at` as said at `now`, both in seconds since 1970: `Now`, `12 min ago`, `3 h ago`,
/// `Yesterday`, `4 days ago`, then `2 Nov`, with its year where that is not this one, its day
/// the one in the zone `offset` says.
pub(crate) fn ago(at: u64, now: u64, offset: UtcOffset) -> String {
    let passed = now.saturating_sub(at);
    match passed {
        _ if passed < MINUTE => "Now".to_owned(),
        _ if passed < HOUR => format!("{} min ago", passed / MINUTE),
        _ if passed < DAY => format!("{} h ago", passed / HOUR),
        _ if passed < 2 * DAY => "Yesterday".to_owned(),
        _ if passed < WEEK => format!("{} days ago", passed / DAY),
        _ => date(local(at, offset), local(now, offset)),
    }
}

/// `at` on the clock of the zone `offset` says, in seconds since 1970 there.
fn local(at: u64, offset: UtcOffset) -> u64 {
    at.saturating_add_signed(offset(at))
}

/// The date of `at`, with its year where it is not `now`'s, both on the user's clock.
fn date(at: u64, now: u64) -> String {
    let (year, month, day) = civil_date(at / DAY);
    let (this_year, _, _) = civil_date(now / DAY);
    let month = MONTHS[month as usize - 1];
    if year == this_year {
        format!("{day} {month}")
    } else {
        format!("{day} {month} {year}")
    }
}

/// The year, the month from 1 and the day from 1 of the day `days` after 1 January 1970, by
/// Howard Hinnant's `civil_from_days`.
fn civil_date(days: u64) -> (u64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 13 November 2026, 12:00 UTC.
    const NOW: u64 = 1_794_571_200;

    fn utc(_at: u64) -> i64 {
        0
    }

    /// Honolulu, ten hours west of UTC all year.
    fn honolulu(_at: u64) -> i64 {
        -10 * 3600
    }

    #[test]
    fn a_recent_moment_is_said_by_how_long_ago() {
        assert_eq!(ago(NOW - 5, NOW, utc), "Now");
        assert_eq!(
            ago(NOW + 90, NOW, utc),
            "Now",
            "a feed's clock ahead of ours"
        );
        assert_eq!(ago(NOW - 12 * MINUTE, NOW, utc), "12 min ago");
        assert_eq!(ago(NOW - 3 * HOUR, NOW, utc), "3 h ago");
        assert_eq!(ago(NOW - 30 * HOUR, NOW, utc), "Yesterday");
        assert_eq!(ago(NOW - 4 * DAY, NOW, utc), "4 days ago");
    }

    #[test]
    fn an_older_moment_is_said_by_its_date() {
        assert_eq!(ago(NOW - 11 * DAY, NOW, utc), "2 Nov");
        assert_eq!(ago(NOW - 400 * DAY, NOW, utc), "9 Oct 2025");
        assert_eq!(civil_date(0), (1970, 1, 1));
        // 05:00 UTC on 2 November is still 1 November in Honolulu.
        assert_eq!(ago(NOW - 11 * DAY - 7 * HOUR, NOW, honolulu), "1 Nov");
        assert_eq!(civil_date(NOW / DAY), (2026, 11, 13));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
    }
}
