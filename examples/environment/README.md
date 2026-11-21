# environment

A program that says the variables its environment holds, one a line as `NAME=VALUE`, in the order of their names, and `none` where it holds none: `TMPDIR`, which WaOS sets, and what a `waos run --env NAME=VALUE` added to that run. What the engine's tests read a run's variables from.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip3/debug/environment.wasm`, from `cargo build -p environment --target wasm32-wasip3`.
- **Used by:** the engine's tests and the app's (`apps/waos/src/logic/dev_server.rs`), through `crates/waos/tests/programs`.
