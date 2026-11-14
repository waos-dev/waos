//! A system other than macOS: it keeps no program's notifications in its settings, since a
//! program has no app of its own there, and its notifications are WaOS's. And, not built yet,
//! nothing hears the user's time zone change on Android, Windows or Linux.

use waos_package::ProgramId;

/// No page of the system's settings holds a program's notifications.
pub(crate) fn notification_settings_link(_program: &ProgramId) -> Option<String> {
    None
}

/// What hears the user's time zone change; nothing does on these systems yet.
pub(crate) enum TimeZoneChanges {}

/// None: a change of the user's time zone is not heard here yet, so programs keep the zone WaOS
/// read as it started until WaOS starts again. Each system announces it its own way, which is not
/// built: Android with the `ACTION_TIMEZONE_CHANGED` broadcast, which only a Java
/// `BroadcastReceiver` hears and the app has no Java of its own; Windows with `WM_TIMECHANGE` and
/// `WM_SETTINGCHANGE` to a top-level window, whose loop is Waft's; Linux with systemd-timedated's
/// `Timezone` property on D-Bus, or a change of the `/etc/localtime` link.
pub(crate) fn watch_time_zone(
    _changed: impl Fn() + Send + Sync + 'static,
) -> Option<TimeZoneChanges> {
    None
}
