# forever

A program that never returns: it counts, and keeps counting, printing `count <n>` every 2^20 steps and asking nothing else of the platform. What the engine's tests stop from outside, and what they see a freeze on: the reports stop, and go on from the same count after the thaw.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip3/debug/forever.wasm`, from `cargo build -p forever --target wasm32-wasip3`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`.
