# WaOS

WaOS runs apps: WebAssembly components that draw with WebGPU. `crates/waos` is the engine any app can embed, `apps/waos` the WaOS app, `wit/` the interface apps are built against, `sdk/wasdk` what they are written with. How it works: `docs/essentials.md`.

The code calls an app a "program", the older word; rename it to "app" in what a change touches anyway.

## Commands

```bash
cargo build                                        # what runs on this machine; no app
cargo test --workspace
cargo clippy --workspace --all-targets
cargo build -p <app> --target wasm32-wasip3        # an app, as a component
wasm-tools component wit wit/                      # checks the WIT package
cargo xtask sync-wit                               # after editing wit/
```

## Rules

- **Changes to this file go to the lead first.** It steers every agent.
- **Edit `wit/`, then sync.** The crates build from their own copy of the interface, so an edit not synced with `cargo xtask sync-wit` is silently ignored.
- **Old apps keep running.** A change to what apps see that an app built before can't run against takes a new interface version; CI refuses it otherwise.
- **The lead settles the WIT before it is written.** It is what every app is built against.
- **`unsafe` only where a system API is wrapped**, so unsafe code stays in a few known places; a crate that needs it denies the lint rather than forbidding it.
- **No `RefCell` unless the problem needs it.** Shared mutable state hides who changes what.
- **An API agents use explains itself.** When an agent has to read the source to use one, its docs or its shape is the bug.
- **Waft comes from crates.io.** To build against an unreleased Waft, a branch adds `[patch.crates-io]` until Waft releases.
- **Work goes on a branch and a pull request**, here and in Waft and Valo; never on `main`. The lead merges.

## Docs

- `docs/essentials.md` holds the app's essential complexity: rules that hold across the code and no single file shows, one line each, saying first what someone sees, then why where it isn't obvious, with a link to the code that keeps it. It changes in the same commit as the code.
- A crate's README says in a sentence or two what the crate is for.
- Plans go in `TODO.md`. There are no other docs.
- Module docs say what the module is for and the invariant an editor could break; comments say only what the code can't.
