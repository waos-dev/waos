# machines

A program, written against the WIT itself, that acts on a machine its user picked for it, in order: it says which machines it holds, asks its user to pick one when it holds none, and says which it holds after; then, on the machine, it runs `/bin/echo` with pipes and says what it printed and how it ended; makes a file in a new folder with a command, with a symlink to it holding its absolute path, and reads it back from the machine's root by its absolute path, through the symlink, and from above `/`; runs `/bin/sh` in a terminal, types a sum and reads the answer, resizes the terminal and reads its new size with `stty`, and types `exit 4`; and starts `/bin/sleep` twice, ends one with `terminate` and the other with `interrupt`, as Ctrl-C would, and says how each ended. It prints each step as `what: result`, and `done` at the end. What a reader learns the machines interface from.

- **Takes:** `wasdk`, the generated bindings.
- **Provides:** a component, `target/wasm32-wasip3/debug/machines.wasm`, from `cargo build -p machines --target wasm32-wasip3`.
- **Used by:** the engine's tests (`crates/waos/tests/all/machines.rs`), through `crates/waos/tests/programs`; whoever runs it in WaOS.
