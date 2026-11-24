# Library

- **The library is one state, shown in two layouts:** Cupertino's below 600 points wide (a phone held upright, a narrow window), WinUI's above. Both show and change the same state. ([library_layout.rs](/apps/waos/src/logic/library_layout.rs), [state::library](/apps/waos/src/state/library/mod.rs))
- **The home lists apps as macOS's Apps does:** A to Z by name, with the apps started most lately in a first row. The row comes from the seven-day record of each app, so an app whose record is off never shows in it. ([often_used.rs](/apps/waos/src/logic/often_used.rs), [home](/apps/waos/src/ui/library/home/mod.rs))
- **The app store shows every source's apps in one place:** Discover searches what the sources list, with what they feature first; Updates lists newer versions; Sources adds and removes sources. An app several sources list shows once, at its newest version. ([app_store.rs](/apps/waos/src/logic/app_store.rs))
  - *Today:* the phone still lists each source with its apps. ([TODO](/TODO.md))
- **What narrows a search shows in the search field:** a category or a source chosen is a token before what's typed, and Backspace takes it back. ([search_field.rs](/apps/waos/src/ui/library/search_field.rs))
- **Opening an app opens it where apps open;** an app already running comes forward. ([state::library](/apps/waos/src/state/library/mod.rs))
- **A package opened with WaOS asks before it installs.** ([state::library](/apps/waos/src/state/library/mod.rs))
- **A tool opens in a terminal of WaOS's own.** ([installed_runs.rs](/apps/waos/src/logic/installed_runs.rs))
  - *Today:* WaOS has no terminal yet, so a tool opened from its icon is refused; one started to be handed files, or by a notification's click, runs. ([TODO](/TODO.md))
