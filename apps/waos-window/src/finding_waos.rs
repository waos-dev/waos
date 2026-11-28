//! The WaOS a window process connects to, as Chrome's app shim finds its browser
//! (`FindOrLaunchChrome`, `PollForChromeReady`): the one answering at the bootstrap name of its
//! data folder; and, for a program's app the user opened alone, from the Dock or the Finder, with
//! none answering, one it starts hidden, then looks for every 100 ms for up to a minute.
//!
//! The invariant: a window process WaOS launched never starts another WaOS. It connects to the
//! one that launched it or to none, so a WaOS that went away does not come back by itself.

use std::io;
use std::path::Path;
use std::thread;
use std::time::Duration;

use waos_window_messages::os::macos::SendRight;

use crate::os;

/// How often a WaOS just started is looked for, and how many times.
const POLL_PERIOD: Duration = Duration::from_millis(100);
const POLL_ATTEMPTS: u32 = 600;

/// How the window process came to be: from WaOS, or from the user alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Launcher {
    Waos,
    User,
}

/// The port of the WaOS answering at `name`; for a window process the user launched, a WaOS
/// started from `waos` when none answers, once it does.
pub(crate) fn find_or_start_waos(
    name: &str,
    waos: &Path,
    launcher: Launcher,
) -> io::Result<SendRight> {
    let looked_up = SendRight::look_up(name);
    if looked_up.is_ok() || launcher == Launcher::Waos {
        return looked_up;
    }
    tracing::info!(waos = %waos.display(), "no WaOS answers: starting one, hidden");
    os::start_waos_hidden(waos)?;
    poll(|| SendRight::look_up(name).ok(), POLL_PERIOD, POLL_ATTEMPTS).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("the WaOS started at {} never answered", waos.display()),
        )
    })
}

/// What `look_up` finds, asked `attempts` times at most, `period` apart.
fn poll<T>(mut look_up: impl FnMut() -> Option<T>, period: Duration, attempts: u32) -> Option<T> {
    for attempt in 0..attempts {
        if attempt > 0 {
            #[allow(
                clippy::disallowed_methods,
                reason = "WaOS just started announces nothing; asked again `period` apart"
            )]
            thread::sleep(period);
        }
        if let Some(found) = look_up() {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_found_at_last_is_answered() {
        let mut asked = 0;
        let found = poll(
            || {
                asked += 1;
                (asked == 3).then_some("waos")
            },
            Duration::ZERO,
            10,
        );
        assert_eq!(found, Some("waos"));
        assert_eq!(asked, 3);
    }

    #[test]
    fn looking_stops_after_the_attempts_allowed() {
        let mut asked = 0;
        let found: Option<()> = poll(
            || {
                asked += 1;
                None
            },
            Duration::ZERO,
            4,
        );
        assert_eq!(found, None);
        assert_eq!(asked, 4);
    }

    #[test]
    fn a_window_process_waos_launched_starts_no_waos() {
        let name = format!("dev.waos.tests.no-waos.{}", std::process::id());
        let found = find_or_start_waos(&name, Path::new("/nonexistent/WaOS.app"), Launcher::Waos);
        assert!(found.is_err());
    }
}
