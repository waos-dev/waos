# clipboard

A program that uses the clipboard, and what a reader learns the `clipboard` interface from. It starts following the clipboard's changes, asks for a window, and waits for what is on the clipboard to change while its window has the focus, `changed: text/plain`; reads the first item in its first type, `read: …`; then copies two items, text given at once with its HTML promised and its RTF promised but never made whole, and a file in its `/data` beside a picture that exists only once pasted, sensitive and expiring after a minute, `written`; and answers each paste of a promised form or file in a task of its own, `asked for: 0 text/html`, with the content and a `done` saying whether it was made whole, which for the RTF says it was not, until another copy replaces its own, `replaced`. Asking to close its window ends it, once it has flushed the clipboard, `flushed`, so the HTML outlives it while the picture ends with it. It prints each step as `what: result`, a refusal by the error's case, `write refused: denied`.

It expects its own `/data`, and a window that gains the focus and is asked to close.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip2/debug/clipboard.wasm`, from `cargo build -p clipboard --target wasm32-wasip2`.
- **Used by:** the engine's tests (`crates/waos/tests/all/clipboard.rs`) and the app's windows' tests, through `crates/waos/tests/programs`.
