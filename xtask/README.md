# xtask

`cargo xtask <task>`: the repository's own chores, run from its checkout, never published.

- `cargo xtask sync-wit` copies `wit/` into each crate that generates bindings from it, replacing the copy there whole, so a file renamed or removed in `wit/` goes from the copies too. Run it after any change to `wit/`; the copies are never edited themselves. Each crate carries a copy, as wasmtime's crates carry theirs (`ci/vendor-wit.sh`), so it builds from what it publishes and the repository checks out on Windows, where Git makes a symbolic link a plain file (the design's decision 57).

- **Takes:** the workspace's checkout: `wit/`, and the crates that carry a copy of it, `sdk/wasdk/wit` and `crates/waos/wit` whole, and `crates/waos-webgpu-host/wit/deps` its `wasi:webgpu`.
- **Provides:** `cargo xtask sync-wit`; `wit_copies::COPIES`, every copy with its source; and `tests/wit_copies.rs`, which fails naming each file of a copy that differs from `wit/`, is missing from a copy, or is in a copy and not in `wit/`.
- **Used by:** whoever changes `wit/`, and `cargo test --workspace`.
