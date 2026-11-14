//! What carries the messages on each system: Mach on macOS (`macos`), where a program's windows
//! are an app of its own. Elsewhere a program's windows are WaOS's own, and nothing crosses.

#[cfg(target_os = "macos")]
pub mod macos;
