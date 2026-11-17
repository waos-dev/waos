//! The user's time zone as the system keeps it, by its IANA name, read through jiff on every
//! system: the `TZ` variable where it is set, then `/etc/localtime`'s link into the zone files on
//! macOS and Linux, Android's `persist.sys.timezone` property, and on Windows its zone mapped to
//! IANA's name by CLDR's table. A zone the system keeps without a name, such as a copied
//! `/etc/localtime`, is none.

/// The IANA name of the user's time zone now; none where the system names none.
pub(crate) fn time_zone() -> Option<String> {
    // jiff keeps the zone it read for five minutes; a change the system announced is read now.
    jiff::tz::reset_system_time_zone_cache();
    let zone = jiff::tz::TimeZone::try_system()
        .inspect_err(|error| tracing::warn!(%error, "no time zone from the system"))
        .ok()?;
    zone.iana_name().map(str::to_owned)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc2_foundation::NSTimeZone;

    /// jiff's reading is the zone Foundation names, which System Settings sets: the same offsets
    /// through a year, so its two names for UTC, Foundation's `GMT` where no zone is set and the
    /// zone files' `UTC`, are one zone.
    #[test]
    fn the_zone_is_the_one_foundation_names() {
        NSTimeZone::resetSystemTimeZone();
        let foundation = NSTimeZone::systemTimeZone().name().to_string();
        let foundation =
            jiff::tz::TimeZone::get(&foundation).expect("Foundation names an IANA zone");
        let name = super::time_zone().expect("the system names its zone");
        let read = jiff::tz::TimeZone::get(&name).expect("jiff names an IANA zone");
        let now = jiff::Timestamp::now();
        for month in 0..12 {
            let instant = now + jiff::SignedDuration::from_hours(24 * 30 * month);
            assert_eq!(
                read.to_offset(instant),
                foundation.to_offset(instant),
                "{name} at {instant}"
            );
        }
    }
}
