# Windows

- **No window until the app asks.** An app gets a view only by requesting one; an app that never asks has none. ([views.wit](/wit/deps/wasdk-platform-2.2.0/views.wit))
- **The shell decides where a view goes:** a window on a desktop, a task on Android, a tab in WaOS's deck. See [desktop](/docs/shell/desktop.md) and [phone](/docs/shell/phone.md). ([ViewPlacement](/crates/waos/src/shell/mod.rs))
- **An app's first request may get a screen already open.** On a phone, an app's screen opens when it launches, empty, and its first request fills it. More requests open more windows wherever the platform can show them. ([view_windows.rs](/crates/waos-program-window/src/state/view_windows.rs))
- **A dialog shows over the app's own window,** or over its screen still waiting for its first request, never over one WaOS guesses. ([view_windows.rs](/crates/waos-program-window/src/state/view_windows.rs))
- **A window fills a screen when its app asks, and the app hears when it leaves.** The system may take a moment, or the user may leave by the system's own means; the window's changes tell the app either way. On a phone, where a view has no window, the app hides the system's bars over it instead. ([windows](/crates/waos/src/windows/mod.rs), [views](/crates/waos/src/views/program_view.rs))
