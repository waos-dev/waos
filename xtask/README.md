# xtask

`cargo xtask <task>`: the repository's own chores.

- **Takes**: `wit/`, read with `wit-parser` as wasmtime's `bindgen!` reads it; rustfmt, from the toolchain.
- **Provides**: `sync-wit`, which copies `wit/` into each crate that generates bindings from it and generates the WIT's value types as plain Rust (`crates/waos-capabilities/src/wit.rs`) with the engine's conversions to its own (`crates/waos/src/wit_conversions.rs`); `check-wit-versions`, which fails when a WIT package changed under the same version.
- **Used by**: whoever changes `wit/`, and CI, which checks both.
