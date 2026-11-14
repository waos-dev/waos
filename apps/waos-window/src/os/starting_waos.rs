//! WaOS started for a program's app the user opened alone, as Chrome's shim starts its browser
//! with no window: hidden, with its launcher closed (`--launcher-closed`), not brought
//! forward, through Launch Services where WaOS is an app bundle, and as a process of its own
//! where it runs outside one, as a development build does.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use objc2_app_kit::{NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSString, NSURL};

/// The argument WaOS keeps its launcher closed at launch with.
const LAUNCHER_CLOSED: &str = "--launcher-closed";

/// Starts the WaOS at `waos`, hidden.
pub(crate) fn start_waos_hidden(waos: &Path) -> io::Result<()> {
    if waos.extension().is_some_and(|extension| extension == "app") {
        open_app_hidden(waos);
        Ok(())
    } else {
        Command::new(waos)
            .arg(LAUNCHER_CLOSED)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
    }
}

/// Opens the app bundle at `app` through Launch Services, hidden and not brought forward. Whether
/// it started is learned by looking for it, as the caller does.
fn open_app_hidden(app: &Path) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&app.to_string_lossy()));
    let configuration = NSWorkspaceOpenConfiguration::configuration();
    configuration.setActivates(false);
    configuration.setHides(true);
    configuration.setAddsToRecentItems(false);
    configuration.setArguments(&NSArray::from_retained_slice(&[NSString::from_str(
        LAUNCHER_CLOSED,
    )]));
    NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
        &url,
        &configuration,
        None,
    );
}
