//! The user's time zone as programs are told it: its IANA name, as the shell reports it, and its
//! rules from the system's time zone database, which answer how far local time is from UTC at
//! any instant (`utc-offset`).
//!
//! The database is jiff's reading of the system's: `/usr/share/zoneinfo` on macOS and Linux,
//! Android's own `tzdata`, and on Windows, which keeps none in IANA's form, the copy jiff bundles.
//! A name the database does not hold is still the program's to read, and its offsets are none.

use jiff::Timestamp;
use jiff::tz::TimeZone;

use wasmtime_wasi::p3::bindings::clocks::system_clock::Instant;

/// One time zone, by its IANA name, with its rules where the database holds them.
#[derive(Clone)]
pub(crate) struct UserTimeZone {
    name: String,
    rules: Option<TimeZone>,
}

impl UserTimeZone {
    /// The zone `name`, its rules looked up in the system's database.
    pub(crate) fn named(name: String) -> UserTimeZone {
        let rules = jiff::tz::db().get(&name).ok();
        if rules.is_none() {
            tracing::warn!(time_zone = %name, "the time zone database does not hold this zone");
        }
        UserTimeZone { name, rules }
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// How far local time is from UTC at `when`, in nanoseconds; none where the database holds
    /// no rules for the zone. An instant outside the years the database's arithmetic covers,
    /// -9999 to 9999, is answered as at the nearest it covers.
    pub(crate) fn utc_offset(&self, when: Instant) -> Option<i64> {
        let rules = self.rules.as_ref()?;
        let offset = rules.to_offset(timestamp_of(when));
        Some(i64::from(offset.seconds()) * 1_000_000_000)
    }
}

/// `when` as jiff's timestamp, within the range jiff represents.
fn timestamp_of(when: Instant) -> Timestamp {
    let seconds = when
        .seconds
        .clamp(Timestamp::MIN.as_second(), Timestamp::MAX.as_second());
    Timestamp::new(seconds, 0).expect("seconds clamped to jiff's range")
}

/// Two zones are the same when their names are: what a change of the user's zone is told from.
impl PartialEq for UserTimeZone {
    fn eq(&self, other: &UserTimeZone) -> bool {
        self.name == other.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600 * 1_000_000_000;

    fn instant(seconds: i64) -> Instant {
        Instant {
            seconds,
            nanoseconds: 0,
        }
    }

    #[test]
    fn paris_is_an_hour_east_in_winter_and_two_in_summer() {
        let paris = UserTimeZone::named("Europe/Paris".to_owned());
        // 2026-01-15T12:00:00Z and 2026-07-15T12:00:00Z.
        assert_eq!(paris.utc_offset(instant(1_768_478_400)), Some(HOUR));
        assert_eq!(paris.utc_offset(instant(1_784_116_800)), Some(2 * HOUR));
    }

    #[test]
    fn new_york_is_west_of_greenwich() {
        let new_york = UserTimeZone::named("America/New_York".to_owned());
        assert_eq!(new_york.utc_offset(instant(1_768_478_400)), Some(-5 * HOUR));
    }

    #[test]
    fn a_zone_the_database_does_not_hold_has_no_offset() {
        let nowhere = UserTimeZone::named("Nowhere/Atlantis".to_owned());
        assert_eq!(nowhere.name(), "Nowhere/Atlantis");
        assert_eq!(nowhere.utc_offset(instant(0)), None);
    }

    #[test]
    fn an_instant_past_the_databases_years_is_answered_at_the_nearest() {
        let paris = UserTimeZone::named("Europe/Paris".to_owned());
        assert!(paris.utc_offset(instant(i64::MAX)).is_some());
        assert!(paris.utc_offset(instant(i64::MIN)).is_some());
    }
}
