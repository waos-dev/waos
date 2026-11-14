# opener

A program, written against the WIT itself, that opens what it names in the apps the system picks, in order: a web link, which opens freely; two links of `zoommtg`, a scheme of its own, of which the first asks its user and the second opens as the user answered; two links of `tel`, the same way; a link of a scheme no app opens; text that is no link; a note it writes in `/data`, opened, then shown in the system's file manager; and a file it does not have. Then it listens for what its user opens with it, until it is stopped: each file it reads and writes to, and each link it says. It prints each step as `what: result`, the result `ok`, `denied`, `no app` or `failed (why)`. What a reader learns the opener interface, and `lifecycle`'s open requests, from.

- **Takes:** `wasdk`, the generated bindings.
- **Provides:** a component, `target/wasm32-wasip2/debug/opener.wasm`, from `cargo build -p opener --target wasm32-wasip2`.
- **Used by:** the engine's tests (`crates/waos/tests/all/opener.rs`), through `crates/waos/tests/programs`; whoever runs it in WaOS.
