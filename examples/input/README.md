# input

A program that says what input its window hears: it asks for a window titled `Input`, draws it in one colour, and says each pointer event, key and change of its text it hears, one line each, as `what: details`. Each press, each scroll and each change of its text draws the window in the next of four colours; a key draws nothing new, and is left unhandled. Its first press starts editing its text, empty, so an input method's text reaches it as the change it makes. Asking to close its window closes it, and the program ends with its view. A change of its size draws it again at that size. What a reader sees `cargo waos tap`, `type`, `key` and `scroll` arrive as, `cargo waos resize` draw it at a size, and `cargo waos screenshot` change with.

- **Takes:** `wasdk`, `wasdk-wgpu`, `wgpu`.
- **Provides:** a component, `target/wasm32-wasip2/debug/input.wasm`, from `cargo build -p input --target wasm32-wasip2`.
- **Used by:** the engine's tests (`crates/waos/tests/all/input.rs`) and the app's (its dev server's), through `crates/waos/tests/programs`; whoever drives it from `cargo waos`.
