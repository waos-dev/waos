# terminal

Terminal: a terminal emulator as a Waft app over `waft-embedder-wasdk`. Each window runs the user's login shell in a pseudo-terminal on a machine the user picked for the program, through WaOS's machines interface, and draws its screen as a grid of cells in the platform's monospace font. It is what a person uses to try what WaOS gives a program that runs commands: a machine picked once and kept, a command in a terminal with its output, input, size and end, the keyboard in full, the fonts, the clipboard, and a window for each session.

- **A machine.** The first run shows a page that says what a machine is and a button that asks WaOS's picker for one; from then on a window starts its shell at once on the machine the program holds. The shell is `/bin/sh -c 'exec "${SHELL:-/bin/sh}" -l'`, so the user's own shell as a login shell, with `TERM=xterm-256color`.
- **The screen** is kept by the `vt100` crate (the grid, the escape sequences, 10 000 lines of scrollback); the app answers the questions it leaves to its host (where the cursor is, what terminal this is), follows the title the shell sets, and resizes the terminal with the window.
- **Typing.** Control, the arrows, the function keys and the editing keys are sent as xterm sends them, the arrows as the program's cursor mode asks; Alt with Left and Right moves by a word; text, an input method's included, goes in through the view's text input, and what is being composed shows at the cursor. Option types characters on a Mac and is Meta elsewhere.
- **The pointer.** A drag selects, a double click a word, a triple click a line; the wheel scrolls the scrollback, or sends the arrow keys to a full-screen program; a program that asked for the mouse gets the presses, drags and wheel, and Shift keeps them for selecting. A finger's drag scrolls.
- **The menus.** Shell: New Window (⌘N), Close Window (⌘W). Edit: Copy, Paste, Select All, the platform's, and Clear (⌘K). View: Bigger, Smaller, Default Size. Control-Shift-C and Control-Shift-V copy and paste where there is no Command key.
- **The end.** A shell left with `exit` closes its window; one that ended otherwise, or did not start, leaves its screen with a line that says so, and Enter starts it again.

It follows the system's light or dark look, with a palette for each.

- **Takes:** `wasdk` (`machines`, `fonts`); `waft-embedder-wasdk` and Waft's crates (`waft-widgets`, `waft-rendering` for `CustomPainter`, `waft-services`, `waft-gestures`, `waft-foundation`, `waft-embedder`), `waft-winui` for the page that asks for a machine; `vt100` (MIT) for the screen.
- **Provides:** a component, the program `dev.waos.terminal`.
- **Used by:** whoever runs it in WaOS.

```sh
cargo waos run -p terminal --release
cargo test -p terminal        # the logic, on this computer
```

Run it with `--release`: a debug component of a Waft app is five times the size, and WaOS takes that much longer to compile it.

## What it leaves out

- **Alacritty's terminal.** `alacritty_terminal` does not build for `wasm32-wasip2` (its pseudo-terminal and poll loop are not optional), so the screen is `vt100`'s. Lines are not rewrapped when the window's width changes, and blinking, strikethrough, hyperlinks and sixel are not drawn. `screen.rs` is the one module that names the emulator.
- **Machines other than Unix ones.** A machine does not say what system it runs or which shell its user has; a Windows machine refuses `/bin/sh`.
- **Menus drawn in the window** where the platform has no menu bar: there the commands are only their keys, and New Window, Clear and the text's size have none.
- **Selecting by touch**, and scrolling a selection past the window's edge while dragging.
