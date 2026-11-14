# survey

A program that looks around, and what a reader learns the API from: it prints what the user set and the screens, then its views; asks for one when it was given none, with no title bar of macOS's own; keeps each view's window between a smallest and a largest size where its user resizes it; treats a band at the top of each view as its title bar, as a program that draws its own does, a press there moving the window and a double-click doing what the system's settings say; follows each view's events, closing its window when the user asks, and saying when its window is maximized or no longer; logs each change of its own state; answers each request to end with `exit`, having nothing to save; and logs every change among its views until the last is gone.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip2/debug/survey.wasm`, from `cargo build -p survey --target wasm32-wasip2`.
- **Used by:** whoever reads it; the engine runs it from step 4 of the design on.
