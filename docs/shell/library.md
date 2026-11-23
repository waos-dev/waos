# Library

- **The library is one state, shown in two layouts:** Cupertino's below 600 points wide (a phone held upright, a narrow window), WinUI's above. Both show and change the same state. ([library_layout.rs](/apps/waos/src/logic/library_layout.rs), [state::library](/apps/waos/src/state/library/mod.rs))
- **Opening an app opens it where apps open;** an app already running comes forward. ([state::library](/apps/waos/src/state/library/mod.rs))
- **A package opened with WaOS asks before it installs.** ([state::library](/apps/waos/src/state/library/mod.rs))
- **A tool opens in a terminal of WaOS's own.** ([installed_runs.rs](/apps/waos/src/logic/installed_runs.rs))
  - *Today:* WaOS has no terminal yet, so a tool opened from its icon is refused; one started to be handed files, or by a notification's click, runs. ([TODO](/TODO.md))
