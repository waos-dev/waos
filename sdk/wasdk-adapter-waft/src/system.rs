//! What `system` says, as Waft's vocabulary: the operating system as the platform whose
//! conventions the widgets follow, light or dark, and the user's languages. Both sides speak
//! Flutter's names, so each is a rename.

use waft_embedder::{Brightness, Locale, TargetPlatform};
use wasdk::system::{self, OperatingSystem};

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
}
