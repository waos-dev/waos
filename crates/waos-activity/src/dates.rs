//! When something happened, as people say it: a date, and how long ago against now.

/// A day, in seconds.
const DAY: i64 = 24 * 60 * 60;

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

/// The date `at`, in seconds since the epoch, is on, in UTC: `15 November 2025`.
pub(crate) fn date(at: i64) -> String {
    let (year, month, day) = civil_from_days(at.div_euclid(DAY));
    let month = usize::try_from(month - 1).map_or("", |month| MONTHS[month]);
    format!("{day} {month} {year}")
}

/// When `at` was, against `now`, both in seconds since the epoch: `just now`, `2 minutes ago`,
/// `3 hours ago`, `yesterday`, or the date for longer ago.
pub fn when(at: i64, now: i64) -> String {
    let ago = (now - at).max(0);
    match ago {
        0..60 => "just now".to_owned(),
        60..3600 => ago_in(ago / 60, "minute"),
        3600..DAY => ago_in(ago / 3600, "hour"),
        _ if ago < 2 * DAY => "yesterday".to_owned(),
        _ => format!("on {}", date(at)),
    }
}

fn ago_in(n: i64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{n} {unit}s ago")
    }
}

/// The year, month and day of the day `days` after the epoch: Howard Hinnant's
/// `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_said_against_now() {
        let now = 1_763_164_800;
        assert_eq!(when(now - 5, now), "just now");
        assert_eq!(when(now - 120, now), "2 minutes ago");
        assert_eq!(when(now - 3600, now), "1 hour ago");
        assert_eq!(when(now - DAY - 10, now), "yesterday");
        assert_eq!(when(now - 3 * DAY, now), "on 12 November 2025");
        assert_eq!(date(0), "1 January 1970");
    }
}
