//! The program's app brought forward as what the user must see shows there, a window that takes
//! the focus: AppKit's `-[NSApplication activate]`, which on macOS 14 and later asks the system,
//! and is honoured when the app in front yielded its activation to this one, as WaOS yields it as
//! it asks for what the user must see (macOS's cooperative activation); before macOS 14,
//! `-[NSApplication activateIgnoringOtherApps:]`. WaOS launches the app in the background, so
//! nothing else brings it forward but the user.
//!
//! The invariant: AppKit's application is reached on the main thread only.

use objc2::{MainThreadMarker, available};
use objc2_app_kit::NSApplication;

/// Asks to bring the app forward; nothing off the main thread.
pub(crate) fn come_forward() {
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(main_thread);
    if available!(macos = 14.0) {
        app.activate();
    } else {
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}
