# Desktop

- **On a desktop, WaOS is a launcher that comes and goes,** as Spotlight does. Its Dock or taskbar icon, its tray icon, or starting it again summons it; Escape, a click outside, or opening an app dismisses it. ([launcher_rules.rs](/apps/waos/src/logic/launcher_rules.rs))
- **On macOS, each running app is an app of its own,** with its own Dock icon, windows and menu bar, as Chrome's app shims are; its code still runs in WaOS. ([program_apps](/apps/waos/src/logic/program_apps/mod.rs))
  - *Why:* the user finds, switches and quits WaOS apps as they do native ones.
- **If an app's window process crashes, the app doesn't hang:** whatever it was waiting on is answered (no window, a dialog cancelled). ([window_process.rs](/apps/waos/src/logic/program_apps/window_process.rs))
- **On Windows and Linux, each installed app has its own entry** in the Start menu or the app list, so the user finds and pins it by its name. ([program_entries](/apps/waos/src/logic/program_entries/mod.rs))
- **WaOS can stay out of the Dock and the taskbar,** with an icon in the menu bar or the tray instead. ([waos_tray.rs](/apps/waos/src/state/waos_tray.rs))
  - *Today:* on macOS its Dock icon stays, since Waft can't make it an accessory app yet. ([TODO](/TODO.md))
- **Quitting WaOS asks every running app,** and WaOS quits only if all agree. An app that never answers keeps WaOS open until it's stopped. ([quitting.rs](/apps/waos/src/state/quitting.rs))
