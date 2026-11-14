# gallery

Waft's Cupertino gallery as a program: an index of entries, each opening one screen that demonstrates one Cupertino widget. The gallery is `cupertino-gallery`, the crate Waft's own binary runs, unchanged; this crate adds the entry, `waft-embedder-wasdk`'s `run`. It asks for its window, and quits when the user closes it. What the engine's tests run an Waft app with, and what the lead looks at in WaOS: `cargo waos run -p gallery`, or, packaged as Gallery with its icon, `icon.png`, `cargo waos package -p gallery`, for WaOS on a phone.

- **Takes:** `cupertino-gallery`, from Waft's checkout beside this one; `waft-embedder-wasdk`; `wasdk`, for the entry.
- **Provides:** a component, `target/wasm32-wasip2/debug/gallery.wasm`, from `cargo build -p gallery --target wasm32-wasip2`; a package, `target/waos/dev.waos.examples.gallery-<version>.wapk`, from `cargo waos package -p gallery`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`; whoever runs it in WaOS.
