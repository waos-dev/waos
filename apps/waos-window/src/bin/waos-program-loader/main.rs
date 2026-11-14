//! The executable of every program's app bundle, a copy of this one, as Chrome copies its
//! `app_mode_loader` into each app shim: it loads WaOS's window library from where the bundle's
//! `Info.plist` says, and hands the process to the library's entry. It holds no program's code,
//! so one copy serves every program, and an update of WaOS changes the window code without
//! rewriting the bundles; the bundle's identity, its `Info.plist`, name and icon, stays the
//! process's, which a loader that started another executable would lose
//! (`../experiments/2026-10-29-generated-app-bundle`).
//!
//! The invariant: the loader stays this small, since each bundle is signed over its copy. A
//! loader that cannot load the library ends at once, which WaOS takes as a failed launch and
//! rebuilds the bundle for.
//!
//! A program's app bundle is macOS's alone; elsewhere the loader builds, so the workspace does,
//! and refuses to run.

#[cfg(target_os = "macos")]
mod os;

use std::process::ExitCode;

#[cfg(target_os = "macos")]
use objc2_foundation::{NSBundle, NSString};
#[cfg(target_os = "macos")]
use waos_window_messages::bundle::{WINDOW_LIBRARY_KEY, WINDOW_MAIN};

#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    eprintln!("waos-program-loader: a program's app bundle, which this loads, is macOS's alone");
    ExitCode::FAILURE
}

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    let Some(library) = window_library() else {
        eprintln!("waos-program-loader: the bundle names no {WINDOW_LIBRARY_KEY}");
        return ExitCode::FAILURE;
    };
    match os::run_entry(&library, WINDOW_MAIN) {
        Ok(status) => ExitCode::from(status as u8),
        Err(reason) => {
            eprintln!("waos-program-loader: {library}: {reason}");
            ExitCode::FAILURE
        }
    }
}

/// Where the window library is, as the bundle's `Info.plist` says.
#[cfg(target_os = "macos")]
fn window_library() -> Option<String> {
    let key = NSString::from_str(WINDOW_LIBRARY_KEY);
    let value = NSBundle::mainBundle().objectForInfoDictionaryKey(&key)?;
    Some(value.downcast::<NSString>().ok()?.to_string())
}
