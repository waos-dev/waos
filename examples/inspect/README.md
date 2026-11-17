# inspect

A program with the inspector role, which it asks for by calling `wasdk::inspect`, wasdk's `inspector` feature: it says this device, each program on it with its icon's size and how many days of its audit summary are kept, then the first running one's mounts, reading one mount's entries and trying a path that leaves it, how many uses it has active, and traces what it writes, saying the first chunk it hears before it returns. WaOS asks its user at its start whether it may see what other programs do, and starts it only with the role granted. What the engine's tests run an inspector as.

- **Takes:** `wasdk`, with its `inspector` feature.
- **Provides:** a component that imports `wasdk:inspect/devices`, `audit`, `active-uses`, `trace` and `storage`.
- **Used by:** the engine's tests, through `waos-test-programs`.

```sh
cargo build -p inspect --target wasm32-wasip3
cargo waos run -p inspect
```
