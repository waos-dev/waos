# hello

The smallest Waft app, in Cupertino: one page that says hello, in the window `run_app` asks for, quitting when the user closes it. It is the program `sdk/waft-embedder-wasdk`'s example completes, and the sample the website's home page shows, read from `src/main.rs` below its module docs, so the site shows what compiles.

- **Takes:** `waft-embedder-wasdk`; Waft's `waft` (the widgets and `run_app`) and `waft-cupertino` (the app and the page), from Waft's checkout beside this one; `wasdk`, for the entry.
- **Provides:** a component, `target/waos/debug/hello.wasm`, from `cargo waos build -p hello`; `cargo waos run -p hello` runs it in WaOS.
- **Used by:** `packages/waos-site`, whose home page shows its code; whoever starts from the smallest program.
