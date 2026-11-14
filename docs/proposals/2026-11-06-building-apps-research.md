# Building apps on WaOS: what two agents hit

Research for the lead, 2026-11-06: two agents each built a real app as a WaOS program, Notes (`examples/notes`) and a video player (`examples/video-player`), on `main` at `5131ab9`, to find what is missing or awkward for a program on macOS and what makes building one slow. No decisions taken. Each line is from the agents' reports, checked against their code where it names it.

## How the building went

- **Most of the work is finding Inset's API, by reading its source.** Notes made 192 tool calls in 35 minutes and the player 237; about two thirds of each were reads and searches, and the largest share of those, 67 and 100, were in Inset's source. WaOS's own side was answered by its WIT doc comments, the proposals and the READMEs, with few reads.
- **Compiling is cheap.** Notes compiled almost first time; the player took three compile-fix rounds. The slowest step is the full workspace test run, which builds every test program as a component (minutes), and `cargo waos build` (about a minute).
- **Nothing an agent can do shows it the app.** Neither could see a window, a dialog, a layout at the smallest size, or a real video playing; both covered logic with unit tests and the engine's headless test shell, and asked for a picture of a frame, a way to find and tap a widget by its label in a test, and a log of the real player's states.
- **A program's panic is lost in tests.** The test shell captures only standard output, so a panic is "wasm trap: unreachable" and a backtrace, without its message.
- **What would have saved the most time,** both said: a guide to Inset for app authors (which crate has what, the common widgets, state, tickers, shortcuts and menus), and a reference app with a menu bar over several windows, a dialog per window, the close and quit flow, and an engine test. Notes and the video player are now two.

## What a program lacks: WaOS

- No reopen: nothing tells a program its Dock icon was clicked while it has no window, so a document app quits with its last window (`lifecycle.wit`).
- A window has no edited mark or represented file, no promise that `show` brings it forward, no full screen a program can enter or read, and no size limits in its `window-config` before its view arrives (`windows.wit`).
- File dialogs cannot offer a system type such as "all movies" (macOS's `public.movie`), only extensions and MIME types; a picked file is mounted at its machine path (`/private/var/folders/…`), and every pick stays mounted for good, with no way to release one (`waos-store`'s `keep_picked`); there is no list of recent documents.
- Context menus have no section headers.
- A program has no system alert or sheet; Inset draws its own.

## What an app lacks: Inset

- Bugs: an empty `PlatformMenuItemGroup` panics (`platform_menu_bar.rs`); `PlatformMenuBar` compares items by identity, so every rebuild sends the whole bar again and can replace a menu the user has open; the default shortcuts bind ← and → to focus moves, so an arrow can never be a menu shortcut (`default_apple_os_shortcuts`); a closed text controller is not freed (`AnyTextEditingController::dispose`).
- Re-exports: `inset-widgets` does not re-export `Ticker`, `FrameCallback`, `HitTestBehavior` or `TapUpDetails`, so an app depends on Inset's inner crates.
- Cupertino has no slider, no popup or menu button, and no macOS-style alert or sheet.
- `PlayerView` never grows past its video's size in points, and its `SubtitleView`'s bottom padding cannot be set.
- No widget sets the window's title, nothing says which window has the focus, and a dialog shown from a menu callback needs a navigator key per window.
- A text field leaves ⌘C and ⌘V to the menu bar's Edit menu, so in a program whose menu bar has no Edit menu nothing pastes, and nothing says why.

## Elsewhere

- `std::env::temp_dir()` panics on `wasm32-wasip2`; both apps read `TMPDIR` themselves.
