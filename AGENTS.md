# waos

A platform for programs that draw with WebGPU. waos is an engine any app can embed, its capabilities, and the WaOS app built on it; wasdk is what programs are written with: the WIT package `wasdk:platform` and its bindings. The design is in `docs/proposals/`: `2026-10-17-the-waos-project.md` and the proposals after it, each with the lead's decisions and their reasons. Read them before changing what a crate is for, and record a new decision there, never here.

## Vocabulary

- **Program**: a WASI component written with wasdk. Not "guest".
- **Hardware buffer**: GPU memory the system's compositor can show (an `IOSurface`, an `AHardwareBuffer`), Waft's `HardwareBuffer`. Not a WebGPU buffer, which is plain GPU memory a program allocates.

## Commands

```bash
cargo build                                        # what runs on this machine; no program
cargo test --workspace
cargo clippy --workspace --all-targets
cargo build -p <program> --target wasm32-wasip3    # a program, as a component
wasm-tools component wit wit/                      # checks the WIT package
cargo xtask sync-wit                               # copy wit/ into the crates that generate bindings, after changing it
```

## Rules

- **Changes to this file go to the lead first.** Something that needs adding to or changing in `AGENTS.md` is shown to the lead and made once they agree; a design decision is recorded in its proposal instead.
- **The WIT is the contract.** A change to what programs see starts in `wit/`, then the engine's bindings and the capability or shell trait that serves it. Never the other way round.
- **Show the interface before writing it.** Every interface, and every capability's details, is shown to the lead as WIT and Rust types with its mental model, and settled, before code is written. `docs/proposals/2026-10-17-waos-v1-interface.md` holds rough designs only.
- **One job per type, and per shell trait.** A type that needs "and" to describe, or a wrapper that only forwards, means the shape is off.
- **Raise what does not fit.** A design that turns out unnatural on some platform, or that needs layers of indirection where the thing should pass through, is raised to the lead with options, never built quietly.
- **Doubt a decision that doesn't make sense.** A recorded decision is the lead's reasoning on its date, not a law. When following one leads somewhere awkward (a workaround, a layer that only forwards, a bug it causes), check whether its reasons still hold for this case, and bring the doubt to the lead with what you found. Don't build around it, and don't quietly override it.
- **Every crate's README says what it takes, what it provides, and who uses it.** A change to either updates it.
- **`unsafe` only where an unsafe API is wrapped**: an `os/` module over a system's API, an implementation of a trait whose owner declares its methods `unsafe`, or a window handle made for a type of ours (`wasdk-wgpu`'s `SharedView`), as winit makes its own. A crate that needs it denies rather than forbids it, and its lint comment says where.

## Code

- A module is named after what it does and holds only that; a function reads at one level of abstraction, its steps' details in functions named after them; a type has one job and does not only forward to another.
- Names are plain nouns for what a value is (`request_id`, `initial_data`); verbs only for actions, and plain ones.
- Diagnostics are `tracing` events and spans that a subscriber reads, never parameters threaded through the types that do the work.

## Docs

Module docs say what a module is for and the invariant a later editor will break. Inline comments only what the next line does that the code cannot say. Do not hard-wrap Markdown. Use plain words, and define a term once.

`docs/essentials.md` is the rules WaOS keeps, one line each with where each is kept: read it before changing how something behaves, and change a rule's line in the same commit as the code that changes the rule.

`DESIGN.md` (once something is built), `docs/` and each crate's README describe what is, and change in the same commit as the code they describe. `docs/proposals/` keeps the designs written before something is built, each with the lead's decisions on top: what was planned on its date, not what is.

## References

Reference checkouts live under `.reference/`, which is not committed. If one is missing, clone it there.

- Tauri, for capabilities as one crate each, with each OS's code inside: `.reference/tauri` and `.reference/tauri-plugins/plugins`.

```
git clone https://github.com/tauri-apps/tauri.git .reference/tauri
git clone https://github.com/tauri-apps/plugins-workspace.git .reference/tauri-plugins
```

Neighbouring repositories:
- `../wasdk`: the wasdk experiment, this project's private predecessor, frozen (not `sdk/wasdk`, the SDK that kept its name). Its code is ported where it is proven, piece by piece with a review, never copied whole; its decisions are precedent, not rules, and where this project's design differs, this project wins. Its `docs/proposals/` and the maintainers' experiments folder (`../experiments/`) hold what it learned.
- `../waft`: Waft (github.com/waos-dev/waft), the framework the WaOS app and `wasdk-adapter-waft` are built on, which this workspace takes from crates.io: a change WaOS needs is released in Waft first, or tried through the README's `[patch.crates-io]`. Changes go there as a branch and a pull request, never on its `main`.
- `../valo`: Valo, Waft's renderer (github.com/seedeai/valo). Changes go there as a branch and a pull request, never on its `main`.

## Workflow

- Commit each finished, checked step on a branch of its own, adding only the paths it changed, once its design has been settled with the lead, and open a pull request for the lead to merge; never push to `main`.
- When a change does not fit the code's shape, reshape the code first, in a commit of its own, then make the change.
- An API or mechanism agents use often explains itself in its docs and its shape. When an agent has to read source to use one, that is a bug to fix, in the docs or in the API; finding and fixing such bugs is ongoing work.
- A design decision that matters goes to the lead, with the options and a recommendation. An implementation question with a measurable answer goes to an experiment under the maintainers' experiments folder (`../experiments/<date>-<question>/`), self-contained, with a README of the question, the commands, the numbers and the conclusion.
