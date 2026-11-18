//! Times as the user's clock shows them, in the time zone Waft's platform gives: the UI says each
//! moment with the `App`, from [`Moment`] which logic keeps as data, and no code here reads the
//! time zone itself.

use waft::{App, DateTime};
use wasdk::system_clock::Instant;

use crate::logic::panels::{Moment, Wording};

/// A day, in seconds.
const DAY: i64 = 24 * 60 * 60;

/// The weekdays' short names, Monday first, as `DateTime::weekday` numbers them.
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// The weekdays' initials, Monday first.
const INITIALS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

/// Now, in seconds since the epoch.
pub(crate) fn now() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
}

/// The start of the day the user is in, as the device keeps its days, in seconds since the epoch:
/// the UTC midnight of the user's calendar date, so the day the report shows as today is the one
/// the user calls today.
pub(crate) fn today(app: &App) -> i64 {
    today_at(app, now_instant())
}

/// `moment` as the user's clock showed it.
pub(crate) fn said(moment: &Moment, app: &App) -> String {
    match moment.wording {
        Wording::Precise => precise(app, moment.at),
        Wording::TimeOfDay => time_of_day(app, moment.at),
        Wording::Ago => said_when(app, moment.at),
    }
}

/// `at` as the user's clock showed it: `14:03:27`.
pub(crate) fn time_of_day(app: &App, at: Instant) -> String {
    let when = local(at);
    format!(
        "{:02}:{:02}:{:02}",
        when.hour(app),
        when.minute(app),
        when.second(app)
    )
}

/// `at` as the user's clock showed it, to the millisecond: `14:03:27.012`.
pub(crate) fn precise(app: &App, at: Instant) -> String {
    format!("{}.{:03}", time_of_day(app, at), local(at).millisecond(app))
}

/// `at` as the user's clock showed it, to the minute: `14:03`.
pub(crate) fn minute_of(app: &App, at: Instant) -> String {
    let mut said = time_of_day(app, at);
    said.truncate(5);
    said
}

/// When `at` was, as the user's clock and calendar tell it: its time today, its weekday this week,
/// else the days since.
pub(crate) fn said_when(app: &App, at: Instant) -> String {
    said_when_on(app, at, today(app))
}

/// The weekday's short name of the user's day `ago` days before today: Mon, Tue…
pub(crate) fn weekday(app: &App, ago: usize) -> &'static str {
    weekday_on(app, ago, today(app), &WEEKDAYS)
}

/// The weekday's initial of the user's day `ago` days before today: M, T, W…
pub(crate) fn weekday_initial(app: &App, ago: usize) -> &'static str {
    weekday_on(app, ago, today(app), &INITIALS)
}

/// The start of the day `at` is in, as the device keeps its days, in seconds since the epoch.
fn today_at(app: &App, at: Instant) -> i64 {
    let when = local(at);
    let date = DateTime::utc(when.year(app), when.month(app), when.day(app));
    date.milliseconds_since_epoch().div_euclid(1000)
}

/// [`said_when`], against the day `today` that `days_of` counts from.
fn said_when_on(app: &App, at: Instant, today: i64) -> String {
    let ago = (today - today_at(app, at)).div_euclid(DAY);
    match ago {
        ..=0 => minute_of(app, at),
        1..=6 => weekday_on(app, usize::try_from(ago).unwrap_or(0), today, &WEEKDAYS).to_owned(),
        _ => format!("{ago} days ago"),
    }
}

/// `names`' of the user's day `ago` days before the day `today`.
fn weekday_on(
    app: &App,
    ago: usize,
    today: i64,
    names: &'static [&'static str; 7],
) -> &'static str {
    let days = i64::try_from(ago).unwrap_or(i64::MAX);
    let date = DateTime::from_milliseconds_since_epoch((today - days * DAY) * 1000, true);
    // `DateTime::weekday` numbers Monday 1 to Sunday 7.
    let weekday = usize::try_from(date.weekday(app) - 1).unwrap_or(0);
    names[weekday]
}

/// `at` as a local [`DateTime`], whose fields read the user's time zone.
fn local(at: Instant) -> DateTime {
    let millis = at.seconds * 1000 + i64::from(at.nanoseconds / 1_000_000);
    DateTime::from_milliseconds_since_epoch(millis, false)
}

/// Now as an [`Instant`], on the program's clock as the moments report it.
fn now_instant() -> Instant {
    Instant {
        seconds: now(),
        nanoseconds: 0,
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use waft::{AppCell, SignedDuration};
    use waft_test::{TestPlatform, TestTimeZone};

    use super::*;

    /// An app whose platform answers `zone` as the user's time zone.
    fn app_in(zone: TestTimeZone) -> Rc<AppCell> {
        let platform = TestPlatform::new().with_time_zone(zone);
        AppCell::with_platform(Rc::new(platform))
    }

    fn at(seconds: i64) -> Instant {
        Instant {
            seconds,
            nanoseconds: 0,
        }
    }

    /// 2026-01-01T23:30:00Z.
    const LATE: i64 = 1_767_310_200;

    #[test]
    fn a_time_reads_in_the_zone_the_platform_gives() {
        let utc = app_in(TestTimeZone::utc());
        let two_hours_on = app_in(TestTimeZone::fixed(SignedDuration::from_hours(2), "CEST"));
        let noon = at(LATE - 11 * 3600 - 30 * 60);
        assert_eq!(time_of_day(&utc.borrow(), noon), "12:00:00");
        assert_eq!(time_of_day(&two_hours_on.borrow(), noon), "14:00:00");
        assert_eq!(
            precise(&two_hours_on.borrow(), noon),
            "14:00:00.000",
            "the millisecond is the instant's, not the zone's"
        );
    }

    #[test]
    fn today_is_the_users_date_and_the_day_before_it_is_the_day_before() {
        let utc = app_in(TestTimeZone::utc());
        let two_hours_on = app_in(TestTimeZone::fixed(SignedDuration::from_hours(2), "CEST"));
        let (utc, two_hours_on) = (utc.borrow(), two_hours_on.borrow());
        assert_eq!(today_at(&utc, at(LATE)), LATE - 23 * 3600 - 30 * 60);
        assert_eq!(
            today_at(&two_hours_on, at(LATE)),
            LATE - 23 * 3600 - 30 * 60 + DAY,
            "01:30 the next day in a zone two hours on"
        );
        assert_eq!(
            said_when_on(&utc, at(LATE), today_at(&utc, at(LATE))),
            "23:30"
        );
        assert_eq!(
            said_when_on(&two_hours_on, at(LATE), today_at(&two_hours_on, at(LATE))),
            "01:30",
            "the same moment is still today, and its time is local, in a zone two hours on"
        );
    }

    #[test]
    fn a_day_this_week_reads_as_the_weekday_of_the_users_date() {
        let utc = app_in(TestTimeZone::utc());
        let utc = utc.borrow();
        let today = at(LATE);
        let today = today_at(&utc, today);
        // 2026-01-01 was a Thursday, and 2025-12-30 was a Tuesday.
        assert_eq!(weekday_on(&utc, 0, today, &WEEKDAYS), "Thu");
        assert_eq!(weekday_on(&utc, 6, today, &WEEKDAYS), "Fri");
        assert_eq!(weekday_on(&utc, 2, today, &INITIALS), "T");
        assert_eq!(
            said_when_on(&utc, at(LATE - 2 * DAY - 3600), today),
            "Tue",
            "two days before Thursday is Tuesday"
        );
        assert_eq!(said_when_on(&utc, at(LATE - 7 * DAY), today), "7 days ago");
    }
}
