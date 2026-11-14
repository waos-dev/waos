//! The symbol Waft's Android host calls once for the process, on the app's own thread, with the
//! process's handle.

// Exported under its own name for Waft's Android host, which `no_mangle` is; that is the `unsafe`
// here.
#![allow(unsafe_code)]

use waft::AndroidApp;

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    crate::android_entry::main(app);
}
