# Desktop

- **On a desktop, WaOS is a launcher that comes and goes,** as Spotlight does. Its Dock or taskbar icon, its tray icon, or starting it again summons it; Escape, a click outside, or opening an app dismisses it. ([launcher_rules.rs](/apps/waos/src/logic/launcher_rules.rs))
- **On macOS, each running app is an app of its own,** with its own Dock icon, windows and menu bar, as Chrome's app shims are; its code still runs in WaOS. ([macos_shell](/apps/waos/src/state/macos_shell.rs))
  - *Why:* the user finds, switches and quits WaOS apps as they do native ones.
- **An app whose window process crashes or is force quit ends with it,** nothing being left to show it; whatever it was waiting on is answered first (no window, a dialog cancelled). ([window_processes](/apps/waos/src/logic/macos_shell/window_processes.rs))
- **A running app can be force quit from its icon in the launcher,** stopped where it stands without being asked: what ends an app that doesn't answer a quit. ([icon_actions](/apps/waos/src/ui/library/icon_actions/mod.rs))
- **On Windows and Linux, each installed app has its own entry** in the Start menu or the app list, so the user finds and pins it by its name. ([program_entries](/apps/waos/src/logic/program_entries/mod.rs))
- **The system offers an app for the kinds of file it opens,** in Open with, never as a type's default, which stays the user's choice: from the app's own bundle on macOS, a ProgID of its own on Windows, its desktop entry's MIME types on Linux, where a kind without one is not offered. ([program_bundle.rs](/apps/waos/src/os/macos/program_bundle.rs), [program_entries](/apps/waos/src/logic/program_entries/mod.rs))
- **WaOS can stay out of the Dock and the taskbar,** with an icon in the menu bar or the tray instead. ([waos_tray.rs](/apps/waos/src/state/waos_tray.rs))
  - *Today:* on macOS its Dock icon stays, since Waft can't make it an accessory app yet. ([TODO](/TODO.md))
- **Quitting WaOS asks every running app,** and WaOS quits only if all agree. An app that never answers keeps WaOS open until it's stopped. ([quitting.rs](/apps/waos/src/state/quitting.rs))
