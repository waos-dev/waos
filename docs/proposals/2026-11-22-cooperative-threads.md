# A program's threads

Written 2026-11-22, while looking at porting Hummingbird, a GPUI music player whose playback, database and search each run on threads of their own. Until now a program had one thread: `std::thread::spawn` answered `Unsupported`, so every port replaced its threads with work in place (Zed's port did it in `sqlez`, `language`'s syntax map, notify's watcher and the `blocking` crate, among others) or with workers. The interface research of 2026-10-17 found that wasmtime runs the component model's cooperative threads behind a flag, and that Rust's `wasm32-wasip3` had no threads yet; decision 10 of `2026-10-17-waos-v1-interface.md` took them "when Rust ships them". The experiment `2026-11-22-std-threads-on-wasip3` (in the maintainers' experiments folder) checked again.

## What the experiment found

- **Rust's own libc still has no threads.** With `nightly-2026-11-15`, a program linked as rustup ships the target gets `Os { code: 58, kind: Unsupported }` from `thread::spawn`, whatever the engine allows. The rustc book expects threads on by default by the end of 2026.
- **wasi-sdk 34's cooperative-threads libc has them.** Linked in its place (three flags), `std::thread` compiles to the component model's thread built-ins: spawn and join, channels, `Condvar`, `park` and `unpark` all work, on Cranelift and on Pulley.
- **They take turns.** Four threads of the same job take 3.98 times one. A thread runs while the others wait: a sleep, a lock, a channel, any call into the host, the executor awaiting. A thread that computes without waiting holds the rest (one tick of a 10 ms ticker in 500 ms).
- **In WaOS's engine, with `wasm_component_model_threading(true)`,** a `#[wasdk::main]` program's thread ran all its 20 ticks while main awaited the clock, and a stop with a thread running ended the program in 2 ms. The engine's 186 tests pass with it on.

## The lead's decisions

1. **The engine runs a program's threads, the component model's cooperative threads** (2026-11-22). It is one setting, and it costs a program that has none nothing. Threads are for porting: crates written around `std::thread` run as written. They take turns on the program's one thread and are never faster; parallel work is still workers (decisions 10 and 11 of `2026-10-17-waos-v1-interface.md`, decision 1 of `2026-11-13-what-six-apps-found.md`). This replaces the wait of decision 10.
2. **A program that wants threads links wasi-sdk 34's cooperative-threads libc itself, until Rust's own has them** (2026-11-22): `-C link-self-contained=no`, the sysroot's library folder, and its `crt1-command.o`, in the program's own Cargo config. Not `waos build` for every program: the sysroot is marked experimental, every program would need wasi-sdk to build, and the flags go once Rust's libc has threads. The guide says how (`docs/guide/threads`).

## Not checked yet

- Freezing a program while one of its threads is in a call into the host.
- Thread-local storage across many threads, and a thread's stack size beyond the default.
