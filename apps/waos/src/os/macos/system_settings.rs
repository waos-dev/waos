//! System Settings at the notifications of a program's app, where macOS keeps whether the app may
//! notify: macOS asks for each app once, and after that only its user changes it there. The link is
//! the one Chrome opens for its own (`base::mac::OpenSystemSettingsPane` with `kNotifications`,
//! which Chrome checks on each macOS from 13 to 26): System Settings' Notifications extension by
//! its bundle id, `com.apple.Notifications-Settings.extension`, whose `Info.plist` lets the
//! `x-apple.systempreferences` scheme open it (macOS 27.0.1), with the app's bundle id as the page
//! to show.
//!
//! The invariant: the page named is the app's bundle id as WaOS writes it (`program_bundle`), so
//! it is the program's app's and no other's.

use waos_package::ProgramId;

use super::program_bundle::bundle_id;

/// System Settings' Notifications, by the bundle id of its extension.
const NOTIFICATIONS: &str = "x-apple.systempreferences:com.apple.Notifications-Settings.extension";

/// The link that opens System Settings at the notifications of `program`'s app.
pub(crate) fn notification_settings_link(program: &ProgramId) -> Option<String> {
    Some(format!("{NOTIFICATIONS}?id={}", bundle_id(program)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_names_the_programs_app() {
        let program: ProgramId = "dev.example.notes".parse().unwrap();
        assert_eq!(
            notification_settings_link(&program).as_deref(),
            Some(
                "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=dev.waos.program.dev.example.notes"
            )
        );
    }
}
