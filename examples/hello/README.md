# hello

The smallest Waft app, in Cupertino: one page that says hello, in the window `run_app` asks for, quitting when the user closes it. It is the program `sdk/wasdk-adapter-waft`'s example completes, and the sample the website's home page shows, read from `src/main.rs` below its module docs, so the site shows what compiles.

- **Takes:** `wasdk-adapter-waft`; Waft's `waft` (the widgets and `run_app`) and `waft-cupertino` (the app and the page), from crates.io; `wasdk`, for the entry.
- **Provides:** a component, `target/waos/debug/hello.wasm`, from `waos build -p hello`; `waos run -p hello` runs it in WaOS.
- **Used by:** `packages/waos-site`, whose home page shows its code; whoever starts from the smallest program.
