# media-assets

A program that reads media without playing it, through the system's reading of media, and what the tests read an asset's way through WaOS from: for each source listed in its `/data/sources`, one a line, in order, it says the media's length, as `source: length 60s`, or `length none` for a live stream; its tracks, each by its kind, its language where it has one, and a `*` where a player would play it, as `source: tracks video*, audio en*`; its title, as `source: title Bip Bop` or `title none`; and the size of its picture a tenth of the way in, or at its start where it has no length, at the nearest keyframe and no larger than 160 by 160, as `source: picture 64x48`; or why one is not, as `source: length failed (refused: why)`. Then it ends. Where the system has no reading of media it says `media assets: none`. Its tests write the sources, and a clip at `/data/clip.mp4`, before it starts.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip3/debug/media-assets.wasm`, from `cargo build -p media-assets --target wasm32-wasip3`.
- **Used by:** the engine's tests (`crates/waos/tests/all/media_assets.rs`) and the app's (`apps/waos/src/state/media_assets.rs`), through `crates/waos/tests/programs`.
