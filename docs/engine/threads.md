# Threads

- **An app's threads run in parallel,** as on any system.
  - *Today:* they take turns on one core, since wasmtime can't share memory between threads yet. Parallel work goes to workers: other instances of the app's code, sharing nothing, exchanging messages over async streams. ([engine.rs](/crates/waos/src/engine.rs), [workers](/crates/waos/src/workers/mod.rs), [TODO](/TODO.md))
