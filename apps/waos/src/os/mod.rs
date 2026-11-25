//! What the app asks of the system itself, beyond Waft's host: the user's time zone, which every
//! system names through jiff (`time_zone`), and each change of it, heard on macOS so far; when
//! this system's apps are frozen (`freeze_policy`); and, on
//! macOS, each program's app of its own: its bundle, its code hash, the key in
//! the Keychain its record is vouched for with, Launch Services, WaOS's activation yielded to it,
//! the layer context its views' layers are hosted from, a program's hardware buffer handed to
//! the main thread, the link to its notifications in System Settings, and Waft's video decoders
//! for a WaOS that is no Waft app; on Android, the entry its
//! activity calls, the app's folders, the system's log, the simulated camera's frames and the
//! programs' shortcuts, through Waft's Android host; on
//! Windows, the user's local application data folder and the Start menu's folder of programs, as
//! the shell names them, and a program's Start menu shortcut; and on each desktop the dev server
//! announced on the local network, as the system announces its services. Each system has a module with the same
//! names for what every system answers, and the target picks the module; a system other than macOS
//! gets the generic answers.
//!
//! The invariant: nothing here names Waft's application or a widget, and `unsafe` is written only
//! in `macos`'s code signature, Launch Services, layer contexts, hardware buffers and time zone
//! changes, which wrap Security's, the system's, Core Animation's, IOSurface's and Foundation's
//! APIs, and in `android`'s entry,
//! folders, log and pattern frames, which wrap the activity's glue, JNI, liblog and hardware buffers, and in `windows`' known folders,
//! shortcuts and dev announcement, which wrap the shell's calls, its COM objects and DNS-SD's.

#[cfg(target_os = "android")]
mod android;
mod freeze_policy;
#[cfg(not(target_os = "macos"))]
mod generic;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod time_zone;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "android")]
pub(crate) use android::*;
pub(crate) use freeze_policy::freeze_policy;
#[cfg(not(target_os = "macos"))]
pub(crate) use generic::*;
#[cfg(target_os = "linux")]
pub(crate) use linux::*;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;
pub(crate) use time_zone::time_zone;
#[cfg(windows)]
pub(crate) use windows::*;
