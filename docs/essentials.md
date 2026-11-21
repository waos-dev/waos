# WaOS: the essentials

The rules WaOS keeps, one line each, so they are not lost under the code. Each links to where it is explained and where it is kept. A change that changes a rule changes its line here in the same commit; a rule that is hard to see in its code is a sign that code needs simplifying.

- [Programs](#programs): what a program is, and the two kinds.
- [Views and windows](#views-and-windows): how a program gets somewhere to draw.
- [Dialogs and questions](#dialogs-and-questions): where they show.
- [Running](#running): when a program runs, waits or freezes.
- [Files](#files): what a program can reach.
- [The interface](#the-interface): what programs are built against.

## Programs

- A program is anything written against wasdk that runs in WaOS. ([decision 3](proposals/2026-11-21-views-a-program-asks-for.md))
- An app has a UI; a tool, a command-line program, has none. A program is an app unless its manifest says `kind = "tool"`. ([decision 3](proposals/2026-11-21-views-a-program-asks-for.md); [`manifest.rs`](../crates/waos-package/src/manifest.rs))
- A `.wapk` is how a program is installed, whether it is an app or a tool. ([`waos-package`](../crates/waos-package/src/lib.rs))
- A program has a role, such as the inspector's, when its component imports that role's interfaces, since that is what lets its code call them; its user grants each role by name. ([`roles.rs`](../crates/waos/src/roles.rs))

## Views and windows

- A program has a view only by asking for one; a program that never asks has none. ([decision 1](proposals/2026-11-21-views-a-program-asks-for.md); [`views.wit`](../wit/deps/wasdk-platform-2.0.0/views.wit))
- The engine passes every request to the shell's placement, which decides where the view goes: a window on a desktop, a task on Android, a tab inside WaOS. ([`ViewPlacement`](../crates/waos/src/shell/mod.rs))
- On a phone, an app's screen opens when it launches, and its first request gets that screen; until then the screen is empty. ([decisions 2 and 10](proposals/2026-11-21-views-a-program-asks-for.md); [`tabs.rs`](../apps/waos/src/state/deck/tabs.rs), [`view_windows.rs`](../crates/waos-program-window/src/state/view_windows.rs))
- A screen waits for its app for as long as the app runs; the user closes it, and it closes when the app ends without asking. ([decision 8](proposals/2026-11-21-views-a-program-asks-for.md))
- A request beyond the first opens another view wherever the platform can show one. In the deck each tab is a window: closing one takes its view away, and closing a program's last tab ends it. ([decision 6](proposals/2026-11-21-views-a-program-asks-for.md))
- On macOS, once a program's app, its window process, is gone, nothing new is asked of it, and everything it was asked, a window the program requested among it, is answered exactly once. ([`window_process.rs`](../apps/waos/src/logic/program_apps/window_process.rs))

## Dialogs and questions

- A dialog or a question goes over the program's own screen: its window, or its screen still waiting for its first request. ([decision 11](proposals/2026-11-21-views-a-program-asks-for.md); [`screen_of`](../crates/waos-program-window/src/state/view_windows.rs))
- On a phone, a program with no screen gets one opened for the dialog, closed once it is answered. Nothing is shown over a screen WaOS guesses. ([decision 11](proposals/2026-11-21-views-a-program-asks-for.md); [`file_dialogs.rs`](../apps/waos/src/state/file_dialogs.rs))
- A tool opened from an icon is refused until WaOS shows a terminal; one started to be handed files or a notification's click runs. ([decision 7](proposals/2026-11-21-views-a-program-asks-for.md); [`installed_runs.rs`](../apps/waos/src/logic/installed_runs.rs))
- Each question is asked once, and a setting the user sets meanwhile answers it. ([`asking.rs`](../crates/waos/src/asking.rs))

## Running

- A program's lifecycle state comes only from how its views stand: resumed while one is on screen and focused, inactive while one is on screen, hidden once none is. WaOS going to the background pauses every program; nothing else sets the state. A program with no view keeps the state it had. ([`state_of_views.rs`](../crates/waos/src/lifecycle/state_of_views.rs), [`lifecycle`](../crates/waos/src/lifecycle/mod.rs))
- A hidden program freezes after a delay, by the freeze rule alone, and never while it is shown, owes an answer, sounds or records. ([`freeze_rule.rs`](../crates/waos/src/lifecycle/freeze_rule.rs))

## Files

- A program reaches no path on the machine outside its mounts, and what is read-only is decided in one table. ([`target.rs`](../crates/waos/src/files/target.rs), [`mounts.rs`](../crates/waos/src/files/mounts.rs))
- A file or folder the user picks for a program, or opens it with, is mounted at its own path until the user takes it back: kept among its grants, which mount it while they hold it, or for its run alone where nothing keeps them. ([`picks.rs`](../crates/waos/src/files/picks.rs))

## The interface

- The WIT is the contract programs are built against; a change a program built before cannot run against takes a new version. ([decision 67](proposals/2026-10-17-the-waos-project.md); `cargo xtask check-wit-versions`)
