//! `wasdk:platform/system`: the operating system the engine runs on, and what its user set: light
//! or dark, the languages, the time zone. What belongs to no one view and no one screen.
//!
//! One [`SystemSettings`] serves every program, written by the shell, and every clone is the same
//! one; the operating system is read from the system itself, and what the user set is the
//! shell's to report, the time zone by its name, whose offsets and abbreviations the engine reads
//! from the system's time zone database ([`time_zone`]). The interface is served to a program by
//! [`host`]. The invariant: a stream hears a change only when the value is another than it was,
//! so the shell may report what it reads as often as it likes.

pub(crate) mod host;
pub(crate) mod time_zone;

use std::sync::{Arc, Mutex};

use futures::channel::mpsc;

use crate::bindings::wasdk::platform::system::{Brightness, Locale, OperatingSystem, SystemChange};
use crate::channel::Channel;
use crate::sync::locked;

use self::time_zone::UserTimeZone;
use wasmtime_wasi::p3::bindings::clocks::system_clock::Instant;

/// The operating system the engine runs on.
pub(crate) fn operating_system() -> OperatingSystem {
    match std::env::consts::OS {
        "android" => OperatingSystem::Android,
        "fuchsia" => OperatingSystem::Fuchsia,
        "ios" => OperatingSystem::Ios,
        "linux" => OperatingSystem::Linux,
        "macos" => OperatingSystem::Macos,
        "windows" => OperatingSystem::Windows,
        _ => OperatingSystem::Other,
    }
}

/// Two locales are the same when every subtag is: what a change of the user's languages is
/// told from.
impl PartialEq for Locale {
    fn eq(&self, other: &Locale) -> bool {
        self.language == other.language
            && self.script == other.script
            && self.region == other.region
    }
}

impl Eq for Locale {}

/// What the platform's user set, as every program sees it: held by the shell to report changes
/// into.
#[derive(Clone)]
pub(crate) struct SystemSettings(Arc<Settings>);

struct Settings {
    brightness: Mutex<Brightness>,
    locales: Mutex<Vec<Locale>>,
    time_zone: Mutex<Option<UserTimeZone>>,
    changes: Channel<SystemChange>,
}

impl SystemSettings {
    /// What the user has set at the start.
    pub(crate) fn new(
        brightness: Brightness,
        locales: Vec<Locale>,
        time_zone: Option<String>,
    ) -> SystemSettings {
        SystemSettings(Arc::new(Settings {
            brightness: Mutex::new(brightness),
            locales: Mutex::new(locales),
            time_zone: Mutex::new(time_zone.map(UserTimeZone::named)),
            changes: Channel::default(),
        }))
    }

    /// The user's light or dark preference now.
    pub(crate) fn platform_brightness(&self) -> Brightness {
        *locked(&self.0.brightness)
    }

    /// The user's languages now, most preferred first.
    pub(crate) fn locales(&self) -> Vec<Locale> {
        locked(&self.0.locales).clone()
    }

    /// The IANA name of the user's time zone now; none where the system has none.
    pub(crate) fn time_zone(&self) -> Option<String> {
        locked(&self.0.time_zone)
            .as_ref()
            .map(|zone| zone.name().to_owned())
    }

    /// How far local time is from UTC at `when` in the user's time zone now, in nanoseconds; none
    /// where the system has no time zone, or the database no rules for it.
    pub(crate) fn utc_offset(&self, when: Instant) -> Option<i64> {
        locked(&self.0.time_zone).as_ref()?.utc_offset(when)
    }

    /// The user's time zone's abbreviation at `when`; none where the system has no time zone, or
    /// the database no rules for it.
    pub(crate) fn time_zone_abbreviation(&self, when: Instant) -> Option<String> {
        locked(&self.0.time_zone).as_ref()?.abbreviation(when)
    }

    /// Each change of what the user set, from now on, on a stream of its own.
    pub(crate) fn changes(&self) -> mpsc::UnboundedReceiver<SystemChange> {
        self.0.changes.subscribe()
    }

    /// The user's light or dark preference is `brightness`; the programs hear of a change.
    pub(crate) fn brightness_changed(&self, brightness: Brightness) {
        let mut current = locked(&self.0.brightness);
        if *current == brightness {
            return;
        }
        *current = brightness;
        // Sent under the lock, so two reports from two threads reach every stream in the order
        // the setting took them.
        self.0
            .changes
            .send(SystemChange::PlatformBrightness(brightness));
    }

    /// The user's languages are `locales`, most preferred first; the programs hear of a change.
    pub(crate) fn locales_changed(&self, locales: Vec<Locale>) {
        let mut current = locked(&self.0.locales);
        if *current == locales {
            return;
        }
        *current = locales.clone();
        self.0.changes.send(SystemChange::Locales(locales));
    }

    /// The user's time zone is the one named `name`, none where the system has none; the
    /// programs hear of a change.
    pub(crate) fn time_zone_changed(&self, name: Option<String>) {
        let mut current = locked(&self.0.time_zone);
        if current.as_ref().map(UserTimeZone::name) == name.as_deref() {
            return;
        }
        *current = name.clone().map(UserTimeZone::named);
        self.0.changes.send(SystemChange::TimeZone(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_brightness_change_is_announced_once() {
        let settings = SystemSettings::new(Brightness::Light, Vec::new(), None);
        let mut changes = settings.changes();
        settings.brightness_changed(Brightness::Dark);
        settings.brightness_changed(Brightness::Dark);
        assert_eq!(settings.platform_brightness(), Brightness::Dark);
        assert!(matches!(
            changes.try_recv().unwrap(),
            SystemChange::PlatformBrightness(Brightness::Dark)
        ));
        assert!(
            changes.try_recv().is_err(),
            "the same value again is not a change"
        );
    }

    #[test]
    fn a_change_of_languages_is_announced_once() {
        let settings = SystemSettings::new(Brightness::Light, Vec::new(), None);
        let mut changes = settings.changes();
        let french = vec![Locale {
            language: "fr".into(),
            script: None,
            region: Some("CA".into()),
        }];
        settings.locales_changed(french.clone());
        settings.locales_changed(french.clone());
        assert_eq!(settings.locales(), french);
        assert!(matches!(
            changes.try_recv().unwrap(),
            SystemChange::Locales(heard) if heard == french
        ));
        assert!(
            changes.try_recv().is_err(),
            "the same languages again are not a change"
        );
    }

    #[test]
    fn a_change_of_time_zone_is_announced_once_and_moves_the_offset() {
        let settings = SystemSettings::new(Brightness::Light, Vec::new(), Some("UTC".to_owned()));
        let mut changes = settings.changes();
        let winter_noon = Instant {
            seconds: 1_768_478_400,
            nanoseconds: 0,
        };
        assert_eq!(settings.utc_offset(winter_noon), Some(0));
        settings.time_zone_changed(Some("Asia/Tokyo".to_owned()));
        settings.time_zone_changed(Some("Asia/Tokyo".to_owned()));
        assert_eq!(settings.time_zone().as_deref(), Some("Asia/Tokyo"));
        assert_eq!(
            settings.utc_offset(winter_noon),
            Some(9 * 3_600 * 1_000_000_000)
        );
        assert_eq!(
            settings.time_zone_abbreviation(winter_noon).as_deref(),
            Some("JST")
        );
        assert!(matches!(
            changes.try_recv().unwrap(),
            SystemChange::TimeZone(Some(name)) if name == "Asia/Tokyo"
        ));
        assert!(
            changes.try_recv().is_err(),
            "the same zone again is not a change"
        );
        settings.time_zone_changed(None);
        assert_eq!(settings.utc_offset(winter_noon), None);
        assert_eq!(settings.time_zone_abbreviation(winter_noon), None);
        assert!(matches!(
            changes.try_recv().unwrap(),
            SystemChange::TimeZone(None)
        ));
    }

    /// The shell reports from its thread and every program's thread reads.
    #[test]
    fn the_settings_cross_threads() {
        fn assert_shared<T: Send + Sync>() {}
        assert_shared::<SystemSettings>();
    }
}
