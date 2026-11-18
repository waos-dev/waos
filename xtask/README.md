# xtask

`cargo xtask <task>`: the repository's own chores, run from its checkout, never published.

- `cargo xtask sync-wit` copies `wit/` into each crate that generates bindings from it, replacing the copy there whole, so a file renamed or removed in `wit/` goes from the copies too. Run it after any change to `wit/`; the copies are never edited themselves. Each crate carries a copy, as wasmtime's crates carry theirs (wasmtime's `ci/vendor-wit.sh`), so it builds from what it publishes and the repository checks out on Windows, where Git makes a symbolic link a plain file (the design's decision 57).
- `cargo xtask sync-docs` copies the site's docs, `packages/waos-site/content`, into `crates/waos-cli/content`, replacing the copy whole, so `cargo waos docs` carries the docs of its own version and the published crate has them. It leaves out the reference's page for each interface, which the site writes from the WIT at each build. Run it after any change to the docs; the copy is never edited itself.

- **Takes:** the workspace's checkout: `wit/`, and the crates that carry a copy of it, `sdk/wasdk/wit` and `crates/waos/wit` whole, and `crates/waos-webgpu-host/wit/deps` its `wasi:webgpu`; `packages/waos-site/content`, and `crates/waos-cli/content`, its copy.
- **Provides:** `cargo xtask sync-wit` and `sync-docs`; `folder_copy::FolderCopy`, a folder copied with what it leaves out, which syncs and names its drift; `wit_copies::COPIES`, every copy of the WIT, and `docs_copy::COPY`, the docs'; and `tests/wit_copies.rs` and `tests/docs_copy.rs`, which fail naming each file of a copy that differs from its source, is missing from the copy, or is in the copy and not in the source.
- **Used by:** whoever changes `wit/` or the docs, and `cargo test --workspace`.
