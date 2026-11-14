//! When a photo was taken, as the photos' page says it: `13 November 2026 at 14:05`, on the
//! user's clock, the moment moved by how far the user's time zone is from UTC at it, which the
//! caller gives (`users_offset`, the system's).

use std::time::{SystemTime, UNIX_EPOCH};

use wasdk::system;
use wasdk::system_clock::Instant;

const SECONDS_IN_A_DAY: i64 = 24 * 60 * 60;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// How far local time is from UTC at a moment, in seconds.
pub(crate) type UtcOffset = fn(SystemTime) -> i64;

/// How far the user's time zone is from UTC at `at`, as the system says; as UTC where the system
/// names no time zone.
pub(crate) fn users_offset(at: SystemTime) -> i64 {
    let instant = Instant {
        seconds: seconds_of(at),
        nanoseconds: 0,
    };
    system::utc_offset(instant).map_or(0, |nanoseconds| nanoseconds / 1_000_000_000)
}

/// `at` in full, in the zone `offset` says: `13 November 2026 at 14:05`.
pub(crate) fn in_full(at: SystemTime, offset: UtcOffset) -> String {
    let local = seconds_of(at) + offset(at);
    let (year, month, day) = civil_date(local.div_euclid(SECONDS_IN_A_DAY));
    let second = local.rem_euclid(SECONDS_IN_A_DAY);
    format!(
        "{day} {} {year} at {:02}:{:02}",
        MONTHS[month - 1],
        second / 3600,
        second / 60 % 60
    )
}

/// `at` in seconds since 1970, negative before it.
fn seconds_of(at: SystemTime) -> i64 {
    match at.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// The year, month from 1 and day of the month of the day `days` after 1 January 1970, in the
/// Gregorian calendar: Howard Hinnant's `civil_from_days`.
fn civil_date(days: i64) -> (i64, usize, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as usize, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// 14:05:30 UTC on 13 November 2026.
    fn afternoon() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_794_578_730)
    }

    fn utc(_at: SystemTime) -> i64 {
        0
    }

    /// Honolulu, ten hours west of UTC all year.
    fn honolulu(_at: SystemTime) -> i64 {
        -10 * 3600
    }

    #[test]
    fn a_photo_is_dated_on_the_users_clock() {
        assert_eq!(in_full(afternoon(), utc), "13 November 2026 at 14:05");
        assert_eq!(in_full(afternoon(), honolulu), "13 November 2026 at 04:05");
        let midnight = afternoon() - Duration::from_secs(14 * 3600 + 5 * 60 + 30);
        assert_eq!(in_full(midnight, honolulu), "12 November 2026 at 14:00");
    }

    #[test]
    fn a_leap_day_and_the_epoch_are_calendar_dates() {
        let leap_day = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(in_full(leap_day, utc), "29 February 2000 at 00:00");
        assert_eq!(in_full(UNIX_EPOCH, honolulu), "31 December 1969 at 14:00");
    }
}
