# composition-layers

A program that places composition layers beneath its drawing, as a player's video or a camera's preview is placed: it makes two layers on the system's compositor, an opaque one and one that is not, and says `layers made`, sets its view's layers to both, the opaque one first, under a transform, a rectangle clip, a rounded-rectangle clip and an opacity, and presents its first frame, then sets them to the other alone, moved, and presents its second, so the first is taken off; where the system has no compositor it says `no compositor` and presents its frames with no layers. It draws only its clear colour, asks for no frame after the second, closes its window when asked, and ends when its view is gone. Nothing presents into its layers, so on screen they show nothing: it is what the engine's tests read the layers each present shows from.

- **Takes:** `wasdk`, `wasdk-wgpu`, `wgpu`.
- **Provides:** a component, `target/wasm32-wasip2/debug/composition-layers.wasm`, from `cargo build -p composition-layers --target wasm32-wasip2`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`; whoever reads it for how a program places its layers.
