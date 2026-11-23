# WaOS: the essentials

The rules WaOS keeps across its code, each with the code that keeps it. A change to a rule changes its line here in the same commit.

## Apps

- **An app, or a tool.** An app has a UI; a tool is a command-line app with none. An app is a tool only when its manifest says `kind = "tool"`. ([manifest.rs](../crates/waos-package/src/manifest.rs))
- **A `.wapk` installs it.** Apps and tools alike install from a signed package. ([waos-package](../crates/waos-package/src/lib.rs))
- **Updates come only from the first publisher.** An update must be signed with the key the app's first install was, whichever source lists it. A source only lists; it vouches for nobody. ([store.rs](../crates/waos-store/src/store.rs))
- **A development install sits beside the published app.** `waos install` installs from source as `<id>.development`, with its own data and settings and " (Development)" after its name. No package can take that id, so no update replaces it. ([store.rs](../crates/waos-store/src/store.rs))
- **The official source comes with WaOS, and can be removed.** `list.waos.app` lists apps released from their own repositories, pinning each one's publisher key. WaOS adds it when the user has never had a list of sources; once removed, it never comes back. ([store.rs](../crates/waos-store/src/store.rs), [state::library](../apps/waos/src/state/library/mod.rs))
- **Built-in apps ship with WaOS.** Each is signed with the WaOS project's key, and this repository is their source. ([release.yml](../.github/workflows/release.yml))
- **A role is what an app's code imports.** An app has a role, such as the inspector's, when its component imports that role's interfaces, since importing is what lets its code call them. The user grants each role by name. ([roles.rs](../crates/waos/src/roles.rs))

## Windows

- **No window until the app asks.** An app gets a view only by requesting one; an app that never asks has none. ([views.wit](../wit/deps/wasdk-platform-2.1.0/views.wit))
- **The shell decides where a view goes:** a window on a desktop, a task on Android, a tab inside WaOS. ([ViewPlacement](../crates/waos/src/shell/mod.rs))
- **On a phone, the screen opens before the app asks.** An app's screen opens when it launches, empty, and its first request gets it. The screen waits as long as the app runs: the user closes it, or it closes when the app ends without asking. ([tabs.rs](../apps/waos/src/state/deck/tabs.rs), [view_windows.rs](../crates/waos-program-window/src/state/view_windows.rs))
- **More requests, more windows, where the platform can show them.** In the deck each tab is a window: closing one takes its view away, and closing an app's last tab ends the app. ([tabs.rs](../apps/waos/src/state/deck/tabs.rs))
- **On macOS, an app's window process that is gone is asked nothing more.** Everything it was already asked, a window the app requested among it, is answered exactly once, so the app never waits on an answer that can't come. ([window_process.rs](../apps/waos/src/logic/program_apps/window_process.rs))

## Dialogs and questions

- **A dialog shows over the app's own screen:** its window, or its screen still waiting for its first request. ([view_windows.rs](../crates/waos-program-window/src/state/view_windows.rs))
- **On a phone, an app with no screen gets one for a dialog,** closed once the dialog is answered. WaOS never shows a dialog over a screen it guesses. ([file_dialogs.rs](../apps/waos/src/state/file_dialogs.rs))
- **A tool opened from its icon is refused** until WaOS has a terminal to show it in. A tool started to be handed files, or by a notification's click, runs. ([installed_runs.rs](../apps/waos/src/logic/installed_runs.rs))
- **Each question is asked once.** A setting the user changes meanwhile answers it. ([asking.rs](../crates/waos/src/asking.rs))

## Running

- **An app's state follows its windows.** Focused window: resumed. On screen but not focused: inactive. None on screen: hidden. WaOS in the background: every app paused. An app with no view keeps the state it had, and nothing else changes it. ([state_of_views.rs](../crates/waos/src/lifecycle/state_of_views.rs), [lifecycle](../crates/waos/src/lifecycle/mod.rs))
- **Out of sight, an app is frozen.** An app with no window on screen is frozen after a short delay, so it uses no CPU or battery. Not while it plays sound, records, or waits on the user's answer: freezing would cut the sound or leave the question hanging. ([freeze_rule.rs](../crates/waos/src/lifecycle/freeze_rule.rs))
- **An app's threads take turns on one core.** Code written for threads runs, just not in parallel; parallel work goes to workers. Wasmtime can't share memory between threads yet. ([engine.rs](../crates/waos/src/engine.rs))

## Files

- **An app reaches only its mounts.** No path on the machine outside them; which are read-only is decided in one table. ([target.rs](../crates/waos/src/files/target.rs), [mounts.rs](../crates/waos/src/files/mounts.rs))
- **A file the user picks stays reachable until they take it back.** A file or folder the user picks for an app, or opens it with, is mounted at its own path: kept among the app's grants while they hold it, or for that run alone where nothing keeps grants. ([picks.rs](../crates/waos/src/files/picks.rs))
- **An app can mount a folder it holds over its own files,** as an editor opens a project at `/`. Its own `/data`, `/cache` and `/tmp` stay on top. ([mounts.wit](../wit/deps/wasdk-platform-2.1.0/mounts.wit), [mount_host.rs](../crates/waos/src/files/mount_host.rs))
- **On Android, This computer is the phone's storage once All files access is on.** WaOS offers to turn it on when This computer is picked, and in its settings; until then This computer's home is WaOS's own folder. ([phone_files.rs](../apps/waos/src/state/phone_files.rs), [all_files_access.rs](../apps/waos/src/os/android/all_files_access.rs))
- **WaOS asks the system only from the main thread.** What the engine needs is read there, kept, and read again when WaOS comes back to the front, where the user returns from changing it. Android ends WaOS when another thread asks. ([state](../apps/waos/src/state/mod.rs), [logic::phone_files](../apps/waos/src/logic/phone_files.rs))

## The interface

- **Old apps keep running.** The WIT is what apps are built against; a change an app built before can't run against takes a new version, and CI refuses it otherwise. (`cargo xtask check-wit-versions`)

## Dev tools

- **The dev tools reach any running app,** however it was installed or started: the dev socket is its user's alone. A run is install, launch, attach and stop; closing an attach leaves the app running. ([dev_server.rs](../apps/waos/src/logic/dev_server.rs))
- **An input from the dev tools is answered once the app has it,** saying whether its window was in sight. Whoever expects a picture waits for it. ([dev_input.rs](../apps/waos/src/logic/dev_input.rs))

## Tests

- **An engine test waits for what it expects, never on the clock.** One deadline, reached only by a broken build. A "nothing happened" check comes after something later did, never after a sleep. ([test_shell](../crates/waos/tests/all/test_shell/mod.rs))
- **Engine tests run side by side.** Each reads only its own shell and its own apps' output. ([test_shell](../crates/waos/tests/all/test_shell/mod.rs))
- **Each system tests what concerns it,** listed in one place. ([ci.yml](../.github/workflows/ci.yml))
