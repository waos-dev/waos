//! WaOS's activation yielded to a program's app, so the app may come forward as what the user must
//! see shows there: macOS's cooperative activation (macOS 14 and later), where only the app in
//! front can let another take its place, and the other then asks for it
//! (`-[NSApplication yieldActivationToApplicationWithBundleIdentifier:]`, then the app's own
//! `-[NSApplication activate]`). Launch Services did both for WaOS when it launched an app
//! brought forward; a program's app is launched in the background now, and takes the activation
//! as its window or dialog shows. While WaOS is not in front the yield does nothing, and the
//! system decides; before macOS 14 there is nothing to yield.
//!
//! The invariant: AppKit's application is reached on the main thread only, and nothing waits on
//! it: the yield is queued there.

use dispatch2::DispatchQueue;
use objc2::{MainThreadMarker, available};
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;
use waos_package::ProgramId;

use super::program_bundle::bundle_id;

/// Yields WaOS's activation to `program`'s app, running or not, on the main thread, if WaOS is in
/// front by then.
pub(crate) fn yield_activation_to(program: &ProgramId) {
    if !available!(macos = 14.0) {
        return;
    }
    let bundle = bundle_id(program);
    DispatchQueue::main().exec_async(move || {
        let Some(main_thread) = MainThreadMarker::new() else {
            return;
        };
        NSApplication::sharedApplication(main_thread)
            .yieldActivationToApplicationWithBundleIdentifier(&NSString::from_str(&bundle));
    });
}
