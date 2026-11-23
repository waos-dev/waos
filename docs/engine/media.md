# Media

- **Media goes through the system's own players and codecs:** playing, cameras, reading media, and decoding and encoding images, so formats and hardware decoding work as in the system's own apps. ([media_players](/crates/waos/src/media_players/mod.rs))
- **The system can draw a video or a camera's picture under the app's drawing,** on a composition layer the app places, with no copy through the app, as a native video view does. ([media_players](/crates/waos/src/media_players/mod.rs))
- **Or the app draws the frames itself:** a frame reader hands over the system's decoded frames, which the app uses as textures with no copy. ([video_frames](/crates/waos/src/video_frames/mod.rs), [gpu_interop](/crates/waos/src/gpu_interop/mod.rs))
- **A source passes the app's own rules:** a link must be reachable on the app's network, a path inside its files. A refused source fails, saying why, and the system's player never sees it. ([media_players](/crates/waos/src/media_players/mod.rs))
- **A camera opens once the user allows it;** "Simulated" gives the app a test pattern instead. ([cameras](/crates/waos/src/cameras/mod.rs))
- **Sound is Web Audio's graph,** every node but AudioWorklet, played on the system's audio. ([audio](/crates/waos/src/audio/mod.rs))
- **Subtitles look as the user set captions on the system.** The app draws them, with the user's caption settings. ([caption_settings](/crates/waos/src/caption_settings/mod.rs))
- **An app's drawing never waits on the screen.** ([hardware_buffers](/crates/waos/src/hardware_buffers/mod.rs))
  - *Why:* the system's compositor keeps each frame's buffer a while after showing it and never says when it's done, so WaOS hands the app a fresh buffer instead of making it wait, as Flutter and Firefox do.
