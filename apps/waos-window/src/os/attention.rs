//! The user's attention asked for the program's app while it is in the background, as it shows a
//! prompt's dialog: AppKit's `-[NSApplication requestUserAttention:]`, whose informational request
//! bounces the app's Dock icon once, in place of bringing the app forward (decision 9 of
//! `docs/proposals/2026-10-29-macos-apps.md`).
//!
//! The invariant: a request is made only while the app is not active, as AppKit's header asks
//! ("call this method only if your application is not already active", `NSApplication.h`), and is
//! cancelled when its holder lets it go; both on the main thread, where AppKit's application is
//! reached.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSRequestUserAttentionType};

/// The user's attention asked for, until this is let go.
pub(crate) struct Attention {
    /// AppKit's number for the request, which cancels it.
    request: isize,
    main_thread: MainThreadMarker,
}

impl Attention {
    /// Asks for the user's attention once, bouncing the Dock icon, while the app is in the
    /// background; none while it is active, or off the main thread.
    pub(crate) fn ask() -> Option<Attention> {
        let main_thread = MainThreadMarker::new()?;
        let app = NSApplication::sharedApplication(main_thread);
        if app.isActive() {
            return None;
        }
        let request = app.requestUserAttention(NSRequestUserAttentionType::InformationalRequest);
        Some(Attention {
            request,
            main_thread,
        })
    }
}

impl Drop for Attention {
    fn drop(&mut self) {
        NSApplication::sharedApplication(self.main_thread).cancelUserAttentionRequest(self.request);
    }
}
