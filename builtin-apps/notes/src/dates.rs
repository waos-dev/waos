//! When a note changed, as the list and the note say it: the list by the time today, `Yesterday`,
//! the day of the week within the week past, and the date before that; the note in full. On the
//! user's clock: each moment moved by how far the user's time zone is from UTC at it, which the
//! caller gives (`users_offset`, the system's), so the days are the user's.

use std::time::{SystemTime, UNIX_EPOCH};

use wasdk::system;
use wasdk::system_clock::Instant;

/// How far local time is from UTC at a moment, in seconds.
pub(crate) type UtcOffset = fn(SystemTime) -> i64;

/// How far the user's time zone is from UTC at `at`, as the system says; none, as UTC, where the
/// system names no time zone.
pub(crate) fn users_offset(at: SystemTime) -> i64 {
    let seconds = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let instant = Instant {
        seconds: i64::try_from(seconds).unwrap_or(i64::MAX),
        nanoseconds: 0,
    };
    system::utc_offset(instant).map_or(0, |nanoseconds| nanoseconds / 1_000_000_000)
}

const SECONDS_IN_A_DAY: u64 = 24 * 60 * 60;

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

/// The days of the week, from the Thursday 1 January 1970 was.
const WEEKDAYS: [&str; 7] = [
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
];

/// `at`, as the list says it `now`, in the zone `offset` says.
pub(crate) fn listed(at: SystemTime, now: SystemTime, offset: UtcOffset) -> String {
    let (at, today) = (Moment::of(at, offset), Moment::of(now, offset).day);
    match today.saturating_sub(at.day) {
        0 => at.time(),
        1 => "Yesterday".to_owned(),
        2..=6 => at.weekday().to_owned(),
        _ => at.short_date(),
    }
}

/// `at` in full, in the zone `offset` says: `6 November 2026 at 14:05`.
pub(crate) fn in_full(at: SystemTime, offset: UtcOffset) -> String {
    let at = Moment::of(at, offset);
    let (year, month, day) = at.date();
    format!("{day} {} {year} at {}", MONTHS[month - 1], at.time())
}

/// A moment on the user's clock, as the day since the Unix epoch it falls on and the second of
/// that day.
struct Moment {
    day: u64,
    second: u64,
}

impl Moment {
    /// `at` moved by `offset`, or the epoch for a moment before it.
    fn of(at: SystemTime, offset: UtcOffset) -> Moment {
        let utc = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let seconds = utc.saturating_add_signed(offset(at));
        Moment {
            day: seconds / SECONDS_IN_A_DAY,
            second: seconds % SECONDS_IN_A_DAY,
        }
    }

    /// `14:05`.
    fn time(&self) -> String {
        format!("{:02}:{:02}", self.second / 3600, self.second / 60 % 60)
    }

    fn weekday(&self) -> &'static str {
        WEEKDAYS[(self.day % 7) as usize]
    }

    /// `6 Nov 2026`.
    fn short_date(&self) -> String {
        let (year, month, day) = self.date();
        format!("{day} {} {year}", &MONTHS[month - 1][..3])
    }

    /// The year, month from 1 and day of the month of the proleptic Gregorian calendar: Howard
    /// Hinnant's `civil_from_days`, for days from the epoch on.
    fn date(&self) -> (u64, usize, u64) {
        let shifted = self.day + 719_468;
        let era = shifted / 146_097;
        let day_of_era = shifted % 146_097;
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
        let year = year_of_era + era * 400 + u64::from(month <= 2);
        (year, month as usize, day)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn utc(_at: SystemTime) -> i64 {
        0
    }

    /// Tokyo, nine hours east of UTC all year.
    fn tokyo(_at: SystemTime) -> i64 {
        9 * 3600
    }

    /// 14:05:30 UTC on Friday 6 November 2026.
    fn friday_afternoon() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_793_973_930)
    }

    fn days_before(at: SystemTime, days: u64) -> SystemTime {
        at - Duration::from_secs(days * SECONDS_IN_A_DAY)
    }

    #[test]
    fn a_change_today_is_listed_by_its_time_and_yesterdays_as_yesterday() {
        let now = friday_afternoon();
        assert_eq!(listed(now - Duration::from_secs(3600), now, utc), "13:05");
        assert_eq!(listed(days_before(now, 1), now, utc), "Yesterday");
    }

    #[test]
    fn the_day_is_the_users() {
        let now = friday_afternoon();
        // 20:00 UTC on Thursday is 05:00 on Friday in Tokyo: today there.
        let thursday_evening = now - Duration::from_secs(18 * 3600 + 5 * 60 + 30);
        assert_eq!(listed(thursday_evening, now, utc), "Yesterday");
        assert_eq!(listed(thursday_evening, now, tokyo), "05:00");
        assert_eq!(
            in_full(friday_afternoon(), tokyo),
            "6 November 2026 at 23:05"
        );
    }

    #[test]
    fn a_change_this_week_is_listed_by_its_day_and_an_older_one_by_its_date() {
        let now = friday_afternoon();
        assert_eq!(listed(days_before(now, 4), now, utc), "Monday");
        assert_eq!(listed(days_before(now, 7), now, utc), "30 Oct 2026");
        assert_eq!(listed(UNIX_EPOCH, now, utc), "1 Jan 1970");
    }

    #[test]
    fn a_change_in_full_says_its_date_and_time() {
        assert_eq!(in_full(friday_afternoon(), utc), "6 November 2026 at 14:05");
        let leap_day = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(in_full(leap_day, utc), "29 February 2000 at 00:00");
    }
}
