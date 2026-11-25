# WaOS

WaOS runs apps: WebAssembly components that draw with WebGPU. `crates/waos` is the engine any app can embed, `apps/waos` the WaOS app, `wit/` the interface apps are built against, `sdk/wasdk` what they are written with. How it works: [docs/waos.md](docs/waos.md).

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
- **Before changing `wit/`, read [the interface's rules](docs/engine/interface.md).**
- **The lead settles the WIT before it is written.** It is what every app is built against.
- **`unsafe` only where a system API is wrapped**, so unsafe code stays in a few known places; a crate that needs it denies the lint rather than forbidding it.
- **Names are the words anyone would use for the thing:** plain nouns for values (`request_id`, `granted_folder`), plain verbs for actions. Never a shorthand that reads only once you know the code: a verb or a loose word made into a type or field (`Pick`, `needs`, `gone_on`, `NotKept`), a phrase made into an identifier (`time_to_finish`), a made-up role (`Keeper`), a verb that says nothing (`tell`).
- **A concept takes the term the systems or a major project already use** (`grace period`, `freeze`, `inhibitor`, `policy`), meaning here what it means there, and one word for it everywhere. A design starts with its terms and where each comes from.
- **No `RefCell` unless the problem needs it.** Shared mutable state hides who changes what.
- **An API agents use explains itself.** When an agent has to read the source to use one, its docs or its shape is the bug.
- **Waft comes from crates.io.** To build against an unreleased Waft, a branch adds `[patch.crates-io]` until Waft releases.
- **Work goes on a branch and a pull request**, here and in Waft and Valo; never on `main`. The lead merges.

## Docs

- `docs/` says how WaOS works, by topic from [docs/waos.md](docs/waos.md) down: each rule one line, saying first what someone sees, a *Why:* where the reason isn't obvious, a *Today:* where the code differs from the intent, and a link to the code that keeps it. Docs change in the same commit as the code they describe.
- **A change to `docs/` or to the website goes to the lead for review before it's made.**
- A crate's README says in a sentence or two what the crate is for. Plans go in `TODO.md`.
- Module docs say what the module is for and the invariant an editor could break; comments say only what the code can't.
