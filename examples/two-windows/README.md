# two-windows

An Waft app with two windows, titled `first` and `second`, each spinning a Cupertino activity indicator, so that each asks for a frame of its own view at every frame it draws. Asking to close either window quits it. What the engine's tests hide one window of, to see that the other keeps drawing, that the hidden one draws nothing, and that it draws again once it shows.

- **Takes:** `wasdk-adapter-waft`; Waft's `waft-widgets` (the windows), `waft-cupertino` (the activity indicator), `waft-services` (quitting), `waft-embedder` and `waft-foundation`, from Waft's checkout beside this one; `wasdk`, for the entry.
- **Provides:** a component, `target/wasm32-wasip3/debug/two-windows.wasm`, from `cargo build -p two-windows --target wasm32-wasip3`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`; whoever runs it in WaOS.
