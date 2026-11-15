# permissions

A program that asks for what it may use, and what a reader learns the `permissions` interface from. It asks for a window, which a question is put over, and goes on without one where the platform gives none, saying `a window` or `no window: …`. It prints where each permission stands, `network: granted`, and follows the changes of each, `network changed: denied`; asks for notifications twice at once, which puts the question to the user once, and prints each answer, `notifications asked: granted`; asks for the network, which opens its page in the program's settings, and prints where it stands once the user leaves the page, `network after its settings: denied`; and returns once it hears its clipboard-write switch change.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip3/debug/permissions.wasm`, from `cargo build -p permissions --target wasm32-wasip3`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`, and the app's tests of the questions it shows.
