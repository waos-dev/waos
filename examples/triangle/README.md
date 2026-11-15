# triangle

The smallest program that draws: one view, one pipeline, a triangle of one colour over the clear colour every frame, drawn the way any wgpu program draws one, on a surface made for its view through `wasdk-wgpu` and configured again at each change of its size. It draws on the view it is given, or asks for one when given none; asks for its next frame after each one it presents, so it draws at the screen's rate; closes its window when asked; and ends when its view is gone. What the engine's tests read a frame back from.

- **Takes:** `wasdk`, `wasdk-wgpu`, `wgpu`.
- **Provides:** a component, `target/wasm32-wasip3/debug/triangle.wasm`, from `cargo build -p triangle --target wasm32-wasip3`.
- **Used by:** the engine's tests, through `crates/waos/tests/programs`; whoever reads it for how a program draws.
