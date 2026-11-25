//! When this system's apps are frozen: the engine's freeze policy, as each system treats an app
//! that left the screen. A phone's system suspends an app seconds after it leaves the screen, so
//! WaOS does the same to its own apps there; a desktop leaves a hidden window at work.

use waos::FreezePolicy;

/// A phone: an app hidden inside WaOS is frozen ten seconds later, as Android freezes one that
/// left the screen, and one WaOS was suspended with has three seconds, under the five iOS gives
/// WaOS itself to finish in.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn freeze_policy() -> FreezePolicy {
    use std::time::Duration;

    FreezePolicy {
        freeze_hidden_after: Some(Duration::from_secs(10)),
        suspend_grace_period: Duration::from_secs(3),
    }
}

/// A desktop: a hidden app runs on, as an app whose window is covered or minimized does on
/// macOS, Windows and Linux, and no system suspends WaOS.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn freeze_policy() -> FreezePolicy {
    FreezePolicy::default()
}
