# wasi-p2

The Preview 2 WIT of what a program's `std` imports for its files on `wasm32-wasip2`, which the engine serves itself (`src/files`): `wasi:filesystem@0.2.12`, and the packages it names, `wasi:io@0.2.12` and `wasi:clocks@0.2.12`. `files.wit` is the world the engine's bindings for it are generated from.

The files under `deps/` are copied unchanged from `wasmtime-wasi` 49.0.3 (`src/p2/wit/deps/`), the same in 50.0.1, the versions `std` and `wasmtime-wasi`'s own WASI agree on; they are WASI's, under the Apache License 2.0 with the LLVM exception, in `LICENSE`, which is `wasmtime-wasi`'s copy. When the standard library moves to WASI 0.3, the engine serves `wasi:filesystem@0.3` over the same core, from `wit/deps/wasi-filesystem-0.3.0`.
