# player

An Waft app that plays Apple's HLS example stream (`https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/master.m3u8`), with three buttons: one that pauses and plays it, one that shows its English subtitles or hides them, and one that switches how the video is shown, on a composition layer the system draws it on, Waft's `PlayerView`, with the subtitles over it, or drawn by the app itself from the frames a frame reader reads, Waft's `VideoFrameView`, with a `SubtitleView` over it. The way is kept in its `/data/view`, `layer` or `frames`, so a run starts as the last was left, on a layer at the first. It prints the player's status each time it changes, `player: loading`, then `player: ready 1920x1080` or `player: failed (why)`, the text track shown, `subtitles: English` or `subtitles: none`, and, ten seconds in, how many of the video's frames it drew itself, `drew 374 video frames in the first ten seconds`, counted from the embedder's events under `wasdk_adapter_waft::targets::VIDEO_FRAMES` by a `tracing` subscriber of its own (`src/frame_count.rs`), none while the system draws the video; where the system has no players, its window says so. Asking to close its window quits it. What the lead looks at to see a program's media play through WaOS.

To run it, start the WaOS built from this workspace, then send the program to it:

```sh
cargo waft run -p waos-app      # builds WaOS.app with the players, and opens it
cargo waos run -p player         # builds the program, and runs it in that WaOS
```

`cargo waos run` starts the installed WaOS.app only when no WaOS answers, and a WaOS built before the players cannot run the program, which imports `media-players`. The program reaches the stream's host, `devstreaming-cdn.apple.com`, under the network switch and its rule of domains, which WaOS checks before it hands the link to the system's player: the defaults, the network on for any domain, let it through; a rule of only some domains needs that host among them, and with the network off the player fails with `the program may not reach devstreaming-cdn.apple.com`. It asks no question of its user.

- **Takes:** `wasdk-adapter-waft`; Waft's `waft-widgets` (the app, its window, `Player`, `PlayerView`, `VideoFrameView` and `SubtitleView`), `waft-cupertino` (the page and the buttons), `waft-embedder` (a player's state, its tracks and its output) and `waft-foundation`, from crates.io; `tracing`, which the frames drawn are counted through; `wasdk`, for the entry and the clock.
- **Provides:** a component, `target/wasm32-wasip3/debug/player.wasm`, from `cargo build -p player --target wasm32-wasip3`.
- **Used by:** whoever runs it in WaOS.
