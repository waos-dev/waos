# video-frames

A program that draws a player's frames itself, and what the tests read a frame's way to a program's GPU from: it makes a frame reader on the system's readers, gives it to a player of `/data/clip.mp4` as its picture's destination and plays it, saying `frames to the reader`; then, in a view of its own, asks the reader at each frame for the frame of that frame's `display-time`, until one comes, and says the frame's size and its first pixel, read back through a texture it imports of its hardware buffer on its own device (`wasdk_wgpu::import_texture`), as `frame 64x48, first pixel [0, 0, 255, 255]`, the bytes as they are in memory. Where the system hands out no frames it says `video frame readers: none`, and where it has no players `media players: none`. Its tests write the clip before it starts, and hand the reader a frame of one colour.

- **Takes:** `wasdk`, `wasdk-wgpu`, `wgpu`, and `futures`, for the buffer's mapping.
- **Provides:** a component, `target/wasm32-wasip2/debug/video-frames.wasm`, from `cargo build -p video-frames --target wasm32-wasip2`.
- **Used by:** the engine's tests (`crates/waos/tests/all/video_frames.rs`) and the app's (`apps/waos/src/state/media_players.rs`), through `crates/waos/tests/programs`.
