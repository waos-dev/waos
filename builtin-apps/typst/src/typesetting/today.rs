//! Today's date, as Typst's `datetime.today()` asks for it: at an offset from UTC a document
//! names, or else on the user's clock, at the offset of the user's time zone now, which the system
//! says (UTC where it names no zone).

use std::time::{SystemTime, UNIX_EPOCH};

use typst::foundations::Datetime;
use wasdk::system;
use wasdk::system_clock::Instant;

const SECONDS_IN_DAY: i64 = 86_400;

/// The date now, `offset_seconds` from UTC, or in the user's time zone where none is given.
pub(crate) fn today(offset_seconds: Option<i64>) -> Option<Datetime> {
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()?;
    let offset = offset_seconds.unwrap_or_else(|| users_offset(now));
    date_of(now + offset)
}

/// How far the user's time zone is from UTC at `seconds` since 1970, in seconds; as UTC where the
/// system names no zone.
fn users_offset(seconds: i64) -> i64 {
    let instant = Instant {
        seconds,
        nanoseconds: 0,
    };
    system::utc_offset(instant).map_or(0, |nanoseconds| nanoseconds / 1_000_000_000)
}

/// The date of the day `seconds` since 1970 falls in.
fn date_of(seconds: i64) -> Option<Datetime> {
    let (year, month, day) = civil_date(seconds.div_euclid(SECONDS_IN_DAY));
    Datetime::from_ymd(year, month, day)
}

/// The year, month and day of the day `days` after 1 January 1970, in the Gregorian calendar:
/// Howard Hinnant's `civil_from_days`, which counts in eras of 400 years from 1 March 0000.
fn civil_date(days: i64) -> (i32, u8, u8) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year as i32, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_since_1970_are_calendar_dates() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(59), (1970, 3, 1));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
        assert_eq!(civil_date(20_770), (2026, 11, 13));
        assert_eq!(civil_date(-1), (1969, 12, 31));
    }
}
