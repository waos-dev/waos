# Build an app on WaOS

Do each step yourself; ask the user only where a step says so.

1. **Tools:** Rust through rustup (ask the user before installing it), then `cargo install waos-cli`.
2. **Make:** `cargo waos new <name>`, a WinUI app that works on desktops and phones; `--template cupertino` only for an app meant for phones alone. Edit `src/main.rs`.
3. **Look things up:**
   - the UI framework: https://waos.app/docs/frameworks/waft (widgets, state, menus, shortcuts, windows);
   - what the platform offers: https://waos.app/docs/guide, one page per capability; everything at once: https://waos.app/llms-full.txt;
   - the APIs on docs.rs: `wasdk`, `wasdk-adapter-waft`, `waft-winui`, `waft-cupertino`.
4. **Icon:** replace `icon.png` with one thing that says what the app does, drawn with care: few colours and one accent, light from above, soft shading, on a quiet background. Aim for the finish of Apple's own icons, and draw it to sit beside WaOS's own apps in its library (they are at https://waos.app/docs/icons); not a line symbol, a letter or a photo. It is 1024 × 1024 and fills the whole square, with no rounded corners, margin, transparency or shadow of its own; WaOS cuts each system's shape, so nothing depends on the edge and what matters stays in the middle two thirds.
5. **Build:** `cargo waos build` until it compiles. A dependency with C code (SQLite, tree-sitter) needs a wasi-sdk on LLVM 23, wasi-sdk 34 or later: https://waos.app/docs/start says how to point the build at it; ask the user before downloading one.
6. **Ship:** `cargo waos package`, which writes a signed `.wapk` under `target/waos/`.

## Trying it, when you want to see it work

- **Run:** `cargo waos run`, left running; it prints the app's output and panics. It needs WaOS installed: if it isn't, ask the user to download it from https://waos.app. An app opened from WaOS itself prints into WaOS's log instead; on Android, which `cargo waos` does not reach, that is `adb logcat -s waos`.
- **Look:** `cargo waos screenshot`, then read the PNG.
- **Resize:** `cargo waos resize --to 390x844` sizes the window to a phone's, in points (a screenshot's pixels over the window's scale), to see the layout at that width; it prints the size the window took.
- **Answer ahead:** `cargo waos set camera allow` (or `simulated`, a test pattern from WaOS on macOS and Android, and `microphone simulated`, a quiet beep), `cargo waos set save-file /tmp/out.pdf`, `cargo waos set machine this-computer`, so a run never stops at a question WaOS asks its user; `cargo waos settings` lists every row, and a run on Ask says on its terminal what WaOS is asking.
- **Act:** `cargo waos tap --at x,y`, `cargo waos press --at x,y` (held a second), `cargo waos drag --from x,y --to x,y` (10 moves over 300 ms), `cargo waos type "…"`, `cargo waos key cmd+s`, `cargo waos scroll --at x,y --by dx,dy`, in the screenshot's pixels; `cargo waos <command> --help` for the rest.
