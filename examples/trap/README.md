# trap

A program that traps: it aborts at once, which in a component is the `unreachable` instruction, as a Rust panic ends in. What the engine's tests see end in failure, without having returned.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip2/debug/trap.wasm`, from `cargo build -p trap --target wasm32-wasip2`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`.
