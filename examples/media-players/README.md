# media-players

A program that plays media through the system's players, and what the tests read a player's way through WaOS from: it opens a player on each source listed in its `/data/sources`, one a line, sends each one's picture to a composition layer of its own, where the system has a compositor, and plays it from two seconds in; then, in the order listed, says each player's state once it no longer loads, as `source: ready 640x360, playing` or `source: failed (why)`; then shows the first player's first text track, where it lists one, saying `text track <id> on`, and says each change of the first player's cues, from those now, as `cues: Hello` or `cues: none`, of its media's title, as `title: Bip Bop`, and of its state, until it is stopped. Where the system has no players it says `media players: none`. Its tests write the sources, and a clip at `/data/clip.mp4`, before it starts.

- **Takes:** `wasdk`.
- **Provides:** a component, `target/wasm32-wasip2/debug/media-players.wasm`, from `cargo build -p media-players --target wasm32-wasip2`.
- **Used by:** the engine's tests (`crates/waos/tests/all/media_players.rs`) and the app's (`apps/waos/src/state/media_players.rs`), through `crates/waos/tests/programs`.
