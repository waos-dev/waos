# inspect

A program with the inspector role, which it asks for by calling `wasdk::inspect`, wasdk's `inspector` feature: it says this device, each program on it with its icon's size and how many days of its audit summary are kept, then traces what the first running one writes and says the first chunk it hears before it returns. WaOS asks its user at its start whether it may see what other programs do, and starts it only with the role granted. What the engine's tests run an inspector as.

- **Takes:** `wasdk`, with its `inspector` feature.
- **Provides:** a component that imports `wasdk:inspect/devices`, `audit` and `trace`.
- **Used by:** the engine's tests, through `waos-test-programs`.

```sh
cargo build -p inspect --target wasm32-wasip3
cargo waos run -p inspect
```
