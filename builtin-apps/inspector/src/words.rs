//! What the app says, in both layouts.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::inspector::Seen;

pub(crate) const APP_NAME: &str = "Inspector";
pub(crate) const PROGRAMS: &str = "Programs";
pub(crate) const LOOKING: &str = "Reading the programs…";
pub(crate) const NO_DEVICE: &str = "WaOS shows Inspector no programs. It needs to be allowed to see what your other programs do, in its settings in WaOS's library.";
pub(crate) const NO_PROGRAMS: &str = "There are no programs on this device.";
pub(crate) const CHOOSE: &str = "Choose a program to see what it did.";
pub(crate) const USED_NOTHING: &str =
    "It has used nothing that needs your permission over the last seven days.";
pub(crate) const LAST_DAYS: &str = "Over the last seven days, on this device";
pub(crate) const RUNNING: &str = "Running";

/// What a program's row says beneath its name: its version, whether it runs, and when it last
/// used something guarded.
pub(crate) fn detail(seen: &Seen) -> String {
    let mut said = Vec::new();
    if let Some(version) = &seen.program.version {
        said.push(version.clone());
    }
    if seen.program.running {
        said.push(RUNNING.to_owned());
    }
    if let Some(last) = seen.activity.last_use {
        said.push(format!("used {}", waos_activity::when(last, now())));
    }
    said.join(" · ")
}

/// What a program used, by name, on one line; empty for nothing.
pub(crate) fn marks(seen: &Seen) -> String {
    seen.activity.marks().join(" · ")
}

/// Now, in seconds since the epoch.
fn now() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
}
