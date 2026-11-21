# Views a program asks for, and programs with none

Written 2026-11-21, after Zed's port started on an Android phone, opened its folder picker, and ended with nothing drawn. Two causes were found. Android's picker answers a document tree with no path, which WaOS refuses: a separate question, see "Not in this proposal". And `wasdk-adapter-gpui` only ever asks for a view, so it never uses the view WaOS gives a program at start on a phone's tab, a home-screen shortcut, a restored card or iOS. That second cause is what this proposal is about. What was planned on this date, not what is.

## What is wrong today

A program gets a view in one of two ways, and which way depends on where it runs, not on the program (decision 4 of `2026-10-17-the-waos-project.md`):

- On a desktop, and for a program's task on Android, WaOS gives nothing at start, and the program asks with `request-view`.
- In a phone's tab, from a home-screen shortcut, for a card Android restores, and in an app bundling one program, WaOS gives a view before the program's code runs. A tab then refuses every request.

Three things follow from this:

- **Every framework adapter has to handle both ways.** `wasdk-adapter-waft` does: it builds the app in the views it was given, or asks when it was given none. `wasdk-adapter-gpui` asks every time, and logs "a view WaOS gave matches no window GPUI opened" for a view it was given. Every adapter for a framework that opens its own windows (GPUI, winit, Bevy) meets this gap.
- **A program with no window has no place.** A command-line program opened in a tab gets a view it never draws in, so the user sees a blank page, and its output goes nowhere they can see.
- **A tab can't hold a second window**, even where the program could show one.

## The lead's decisions

1. **A view exists only because the program asked for it** (2026-11-21). `request-view` is the one way a program gets a view, on every platform and in every launch mode, and WaOS decides where the view goes. A program that never asks has no view, and that is the normal case, not an error. This amends decision 4 of `2026-10-17-the-waos-project.md`: shells no longer give views at start, and a phone's tab no longer refuses requests. Why: it is how a program on a desktop already works, so one program runs the same everywhere, and every framework that opens its own windows works through its adapter's one existing path.

2. **A screen WaOS opens before the program asks is held for the program's first request** (2026-11-21). When the user opens a program from an icon (a tab, a home-screen shortcut, a card Android restores, the one view of a bundled app), WaOS shows that screen at once with the program's launch picture, and gives it to the program's first `request-view`. The view arrives as any requested view does, carrying the request's id, and the request's initial data is the view's own. The program can't tell a held screen from a new window, and a framework needs no code to adopt one. While the screen is held, it is the parent of the program's dialogs, so a program that asks for a file before opening its window (Zed picks a folder first) has its dialog shown over its own screen. A program that ends without asking has its held screen closed, or, in a tab, left showing how it ended, as decision 3 of `2026-11-12-phone-programs-and-library.md` keeps for any ended program.

   A restored card fits here with nothing added. Decision 18 of `2026-11-12-phone-programs-and-library.md` restores only a program's main card, which starts the program fresh, and closes a restored card for an extra view. So a restored main card is a held screen for a fresh launch, like any other, and the platform giving WaOS that card (Waft's side, decision 19 there) stays as it is.

3. **Whether a program has a UI is read from its imports: a component that imports `wasdk:platform/views` has one, and one that doesn't is a console program** (2026-11-21). The rule is read the way roles are (`crates/waos/src/roles.rs`, decision 9 of `2026-11-15-inspector-roles-and-compatibility.md`): from the component alone, the same for an installed package and a development run, with nothing the program declares. Why this signal and no other:
   - Not `.wapk` against a bare `.wasm`. The dev loop sends GUI programs to a phone as bare components, and a packaged command-line tool wants what a package gives (an id, updates, its grants kept between runs).
   - Not wasdk as a whole. A command-line tool may use the clipboard, notifications, file dialogs or machines without ever drawing.
   - Not `wasi:cli`. Almost every Rust program imports stdin and stdout through `std`, GUI programs included.

   wasdk is built so that a component imports only what it calls, so importing `views` means the code reaches for a view. Every framework adapter imports it, so every program built on one is a GUI program.

4. **Where the program is opened from decides what WaOS shows for it**:

   | | Opened from an icon | Run from a terminal (`cargo waos run`, `waos run`) |
   |---|---|---|
   | Imports `views` | A held screen (decision 2) | Nothing held; any view it asks for opens normally; its input and output are the terminal's |
   | Doesn't import `views` | A terminal of WaOS's own for its input and output (decision 5) | The terminal it runs in |

   The case this rule gets wrong is a command-line tool that sometimes opens a window. Opened from an icon, it gets a held screen instead of a terminal, and its text output goes only to the log. That is accepted until a real program needs more.

5. **The terminal is a widget in Waft, so other apps can use it, and it comes after this work** (2026-11-21). WaOS shows it for a console program opened from an icon: in a tab on a phone, in a window of its own on a desktop. Its design is shown to the lead on its own. Until it exists, decision 7 says what happens.

6. **A request beyond the first opens another view wherever the platform can show one** (2026-11-21): in the deck, another tab for the program, as decision 10 of `2026-11-12-phone-programs-and-library.md` makes every request a task; `unsupported` only where the place can hold no more, such as an app bundling one program with its one view.

7. **Until the terminal exists, a console program is refused when opened from an icon** (2026-11-21), with a message that it runs from a terminal. Run from a terminal, it works as it does today.

8. **A held screen waits for as long as its program runs** (2026-11-21), showing the launch picture, and the user closes it when they choose to, which ends the program as closing its window would. WaOS doesn't guess when a program is too slow: a program waiting on the network before its first window is as normal as one that hangs, and the user can tell them apart where WaOS can't.

9. **The interface changes now, without keeping the old shape** (2026-11-21): no program outside this repository depends on it yet.

## The interface

No function is added or removed. The comment on `interface views` in `wit/deps/wasdk-platform-1.0.0/views.wit` would read:

```wit
/// Where a program draws and where its input comes from.
///
/// A view is a rectangle of physical pixels the platform puts on screen: the whole screen in a
/// phone's tab, a window's contents in a window. A program has a view only by asking for one with
/// `request-view`, and hears of each on one stream, `view-changes`: the shell decides where each
/// goes, a window on a desktop, a task on Android, a tab inside WaOS, and may give a request a
/// screen it already shows, such as the one the user opened the program into. A program that
/// never asks has no view. `views` is what the program has now, which is none before it asks. A
/// view carries the id of the request it arrived for, and what the request attached as
/// `initial-data`.
```

And the error on `view-error`:

```wit
variant view-error {
  /// The place the program runs in can show no other view, such as a phone's tab that holds one.
  unsupported,
  failed(string),
}
```

`views()` stays: it answers what the program has now, which is still useful after it has asked. `view.request-id` answers a plain `request-id`, no longer an option, since every view now arrives for a request:

```wit
    /// The request this view arrived for.
    request-id: func() -> request-id;
```

That changes the type, which decision 67 of `2026-10-17-the-waos-project.md` says moves the interface's version; it is changed now, since no program outside this repository depends on it yet (decision 9).

On the shell's side, a held screen is kept by the engine, so every shell gets it the same way. A shell that shows a screen before the program asks starts the program with `WaOS::run_reserving(program, environment, screen)`, where `WaOS::run` starts one with nothing held. The program's first `request-view` takes the screen without reaching `ViewPlacement::view_requested`. The shell waits on the `ReservedView` it was handed, which resolves with the `ProgramView` the screen became, or with none once the program ends without asking. Every later request goes to `view_requested`, which answers with `program.add_view(request, view)`. In the code, a held screen is called a *reserved* one, since `run_held` already names a program held before its first instruction.

## The work, in order

1. In the WaOS app, the deck's tab, a home-screen shortcut's activity and a restored card hold their screen and give it to the first request. They no longer give a view at start (`apps/waos/src/state/deck/tab_view.rs`, `apps/waos/src/state/given_views.rs`, `crates/waos-program-window/src/state/view_windows.rs`'s `open_given`, `given_started`, `given_not_started`).
2. The engine reads whether a component imports `views` beside its roles, and WaOS reads that before choosing what to show for a launch.
3. The views WIT as above, under a new version of `wasdk:platform` (2.0.0, since a type changes), which CI asks for; the `views` docs in `sdk/wasdk`; a second request in a tab opening another tab (decision 6); and a console program refused from an icon (decision 7).
4. `wasdk-adapter-waft` drops its loop over given views and asks for its first view as on a desktop. `wasdk-adapter-gpui` needs no change for this.
5. Zed, rebuilt and run on the phone from the library and from a shortcut.
6. The Waft terminal widget, shown to the lead first (decision 5).

## Not in this proposal

- **Android's document trees.** A folder picked on Android has no path, and WaOS mounts only paths, so `picked_folder_path` refuses the pick (`crates/waos-program-window/src/logic/file_dialogs.rs`). Under consideration: turning a pick in the phone's own storage into its path, when WaOS holds "All files access", mounting that one folder, and failing a cloud folder with a clear message. Designed with the lead on its own.
- **Zed quitting when no folder is picked**, which is the Zed port's own bug: it opens an empty workspace instead.
