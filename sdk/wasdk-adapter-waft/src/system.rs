//! What `system` says, as Waft's vocabulary: the operating system as the platform whose
//! conventions the widgets follow, light or dark, the user's languages, and the local time zone at
//! an instant. Both sides speak Flutter's names, so each is a rename.

use waft_embedder::{Brightness, Locale, SignedDuration, TargetPlatform};
use wasdk::system::{self, OperatingSystem};
use wasdk::system_clock;

/// The platform whose conventions the widgets follow. A system Waft cannot name is treated as
/// Linux, as Waft's winit embedder treats one.
pub(crate) fn target_platform_of(os: OperatingSystem) -> TargetPlatform {
    match os {
        OperatingSystem::Android => TargetPlatform::Android,
        OperatingSystem::Fuchsia => TargetPlatform::Fuchsia,
        OperatingSystem::Ios => TargetPlatform::IOS,
        OperatingSystem::Linux | OperatingSystem::Other => TargetPlatform::Linux,
        OperatingSystem::Macos => TargetPlatform::MacOS,
        OperatingSystem::Windows => TargetPlatform::Windows,
    }
}

pub(crate) fn brightness_of(brightness: system::Brightness) -> Brightness {
    match brightness {
        system::Brightness::Light => Brightness::Light,
        system::Brightness::Dark => Brightness::Dark,
    }
}

/// The user's language with the subtags `system` named.
pub(crate) fn locale_of(locale: system::Locale) -> Locale {
    let mut result = Locale::new(locale.language);
    if let Some(script) = locale.script {
        result = result.script_code(script);
    }
    if let Some(region) = locale.region {
        result = result.country_code(region);
    }
    result
}

/// `seconds_since_epoch` as the instant on the WIT's system clock, whose epoch is the Unix one.
pub(crate) fn instant_of(seconds_since_epoch: i64) -> system_clock::Instant {
    system_clock::Instant {
        seconds: seconds_since_epoch,
        nanoseconds: 0,
    }
}

/// How far local time is from UTC in nanoseconds, as `utc-offset` gives it, as Waft's duration,
/// which counts microseconds.
pub(crate) fn signed_duration_of(nanoseconds: i64) -> SignedDuration {
    SignedDuration::from_microseconds(nanoseconds / 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_system_waft_cannot_name_follows_linux() {
        assert_eq!(
            target_platform_of(OperatingSystem::Other),
            TargetPlatform::Linux
        );
        assert_eq!(
            target_platform_of(OperatingSystem::Linux),
            TargetPlatform::Linux
        );
        assert_eq!(
            target_platform_of(OperatingSystem::Macos),
            TargetPlatform::MacOS
        );
        assert_eq!(
            target_platform_of(OperatingSystem::Ios),
            TargetPlatform::IOS
        );
    }

    #[test]
    fn a_locale_keeps_the_subtags_the_system_named() {
        let locale = locale_of(system::Locale {
            language: String::from("zh"),
            script: Some(String::from("Hant")),
            region: Some(String::from("TW")),
        });
        assert_eq!(locale.to_language_tag(), "zh-Hant-TW");

        let plain = locale_of(system::Locale {
            language: String::from("en"),
            script: None,
            region: None,
        });
        assert_eq!(plain.to_language_tag(), "en");
    }

    #[test]
    fn brightness_is_the_users_own_preference() {
        assert_eq!(brightness_of(system::Brightness::Dark), Brightness::Dark);
        assert_eq!(brightness_of(system::Brightness::Light), Brightness::Light);
    }

    #[test]
    fn a_seconds_since_the_epoch_instant_has_no_nanoseconds() {
        let instant = instant_of(1_768_478_400);
        assert_eq!(instant.seconds, 1_768_478_400);
        assert_eq!(instant.nanoseconds, 0);
    }

    #[test]
    fn an_offset_in_nanoseconds_is_wafts_microseconds_either_side_of_utc() {
        assert_eq!(
            signed_duration_of(9 * 3_600 * 1_000_000_000),
            SignedDuration::from_hours(9)
        );
        assert_eq!(
            signed_duration_of(-5 * 3_600 * 1_000_000_000),
            SignedDuration::from_hours(-5)
        );
        assert_eq!(signed_duration_of(0), SignedDuration::ZERO);
    }
}
