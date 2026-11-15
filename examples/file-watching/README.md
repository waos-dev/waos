# file-watching

A program that watches a folder in its own `/tmp`, in order: it says what `TMPDIR` names and that `/tmp` takes a file; opens `/tmp/watched` from the preopened root and watches it twice, everything under it and its own entries alone; makes a file, writes it, renames it and removes it, saying how each change came; writes a file in a folder beneath, which only the recursive watch tells, and a file beside, which both tell; and says what a watch of the root and of a file fail with. It prints each step as `what: result`, and `done` at the end. What a reader learns the file-watching interface from.

- **Takes:** `wasdk`, the generated bindings.
- **Provides:** a component, `target/wasm32-wasip3/debug/file-watching.wasm`, from `cargo build -p file-watching --target wasm32-wasip3`.
- **Used by:** the engine's tests (`crates/waos/tests/all/file_watching.rs`), through `crates/waos/tests/programs`; whoever runs it in WaOS.
