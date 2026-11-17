# A program's media across the WIT

**Decisions taken (2026-11-02).** The lead took these one question at a time, settling what programs see in the WIT before the APIs inside WaOS and Inset. They carry the native-surfaces decisions (`2026-10-30-native-surfaces.md`) across the WIT, and amend the media decisions of 2026-10-29 (`2026-10-29-media.md`) where named; the principle throughout is the lead's decision 25 there: the interface shows what the platforms do, and hides no pool, copy, limit or lifetime for ergonomics. The research on subtitles is `2026-11-02-subtitles-research.md`.

1. **A program's native content is a `composition-layer` it makes, named with each present it shows under:** `view.present(layers: list<placed-layer>)`, each layer with the native-surfaces decision 6 placement (a rectangle, then transforms, rectangle and rounded clips and opacity, a path clip falling back to its bounds), beneath the program's drawing where it leaves it clear. A layer is made on its own by the system's compositor, `get-compositor()` answering none where the system has none, so a program there draws frames itself (native-surfaces decision 30), and `make-layer(opaque)` (the lead's answer of 2026-11-02, building it, in place of a constructor, which cannot fail), and is used by whatever it is given to, as Inset's `Compositor::make_layer`; a present shows exactly the layers it names, as native-surfaces decision 23 has it, the layers crossing as resources and their placements with the frame they belong to. This replaces media decision 7's `open-surface` and `place`.
2. **A player's or camera's picture goes to one destination at a time, which the program makes: a layer, or a frame reader** (from `get-video-frame-readers()`, none where the system hands out no frames, and `make-reader()`, as layers come from `get-compositor()`: the lead's answer of 2026-11-02, since a constructor cannot fail), given to a player with `set-video-output(option<video-output>)`, and to a camera among its outputs. This is how the native platforms build it: Media3's `setVideoSurface` takes a surface from a `SurfaceView` or from an `ImageReader` the app made, an Apple app makes its own `AVPlayerItemVideoOutput` and adds it to the item, a Windows app gives frame-server mode a surface of its own, and a camera's frames come the same way (`AVCaptureVideoDataOutput`, CameraX's `ImageAnalysis`). It says media decision 5's one destination in the call itself, where `set-video-layer` beside `read-frames` hid it, and gives readers the model layers have. Not the player making the reader and handing it back for `frames` only, Inset's shape before, which returned a value for one case and made a camera's reader differ from a player's. Inset changes the same way.
3. **A frame reaches the program's GPU as the platforms hand one out: the frame, the hardware buffer in it, and one generic conversion,** `video-frame.hardware-buffer()` and `hardware-buffer.texture(device)`. Android's `Image.getHardwareBuffer()` is valid while the image is open and Vulkan imports any `AHardwareBuffer`; Apple's frame is a `CVPixelBuffer` over an `IOSurface`, which Metal makes a texture of; Chrome, inside, keeps every frame and image as a shared image that Dawn imports with one call. The frame holds its slot in the decoder's pool until it, its buffer and their textures are dropped (measured), so the pool is not hidden. A web host backs a `hardware-buffer` with a web `VideoFrame`, which wraps a canvas, an image bitmap or a video's frame alike, and makes its texture with `copyExternalImageToTexture`: `texture()` promises a texture of the buffer for sampling, and sharing memory is what native hosts do. Frames are BGRA, asked of the decoder so; YUV planes, for a program that wants no conversion, come later. Not a frame without the frame object, a bare buffer quietly holding a pool slot and leaving no place for a frame's time, rotation and colour; not a texture straight from the frame, which a second kind of buffer would duplicate.
4. **A frame tells when its picture is expected to reach the screen, `frame.display-time`, beside `time`,** when its refresh began, which animations advance to. A program asks a reader for the frame of its display time (native-surfaces decision 29). Apple's display link gives both (`timestamp`, `targetTimestamp`), as do Android's frame data (its frame time and expected presentation time) and the web (`requestAnimationFrame`'s time and `requestVideoFrameCallback`'s `expectedDisplayTime`).
5. **The player is a `media-player`, in `media-players`, reached through `get-media-players` and `open(source)`**, media decision 7's shape: it plays audio as well as video, as Android's and Windows' `MediaPlayer`, Apple's `AVPlayer` and the web's media element do, so not `video-player`; and a system with no player answers none.
6. **A player tells its media's tracks, plays one audio track and shows one text track or none, and tells the media's tags.** `player-state.tracks` lists each with its kind, language, label and whether it is selected, so a player app knows an MP3 has no picture rather than reading it from a missing size, as every platform lists tracks (`AVPlayerItem.tracks`, Media3's `getCurrentTracks`, Windows' `AudioTracks` and `VideoTracks`, the web's track lists). `select-audio-track` and `select-text-track` follow every platform's one-audio, one-text-or-none rule (Apple's media selection groups, Media3's overrides, Windows' `SelectedIndex`, the web's `enabled` and `mode`); no platform but Media3 chooses among a stream's video versions. `metadata` (title, artist, album, artwork) has a stream of its own, as Media3's `onMediaMetadataChanged` sits beside its other callbacks, so the artwork is not copied with each state; Apple and Media3 give it from the player, Windows from a file's properties, and the web not at all, whose host answers it empty.
7. **The program draws subtitles, from the cues of the selected text track; the system draws none.** The research found that a program gets every subtitle the system would draw, with styling and position, on Apple, Android and Windows, and that most players draw their own: Media3's `SubtitleView` over the video, Safari after `suppressesPlayerRendering`, Chrome, mpv, VLC. A cue is Media3's `Cue`, field for field, since Media3 turns every format it reads into it. WaOS's host merges the user's caption settings into each cue, as Apple's legible output does by default and Chrome's cue style sheet does in CSS: what the user marked as always theirs wins, and the rest fills in where the media says nothing; so no program can forget the people who set it, as a web page drawing its own captions must, with no way to read the setting. Inset gains a subtitle widget that `PlayerView` puts over the video, as Media3's `PlayerView` does. Not the system drawing them, which works only when the system draws the video and has nothing on Android, and would leave frames drawn by the program without subtitles.
8. **A camera lists its formats, and a program starts it in one of them with its outputs given whole:** `camera.formats()` and `start(format, outputs)`, which fails only for what can happen at any time (the camera in use, unplugged, not allowed). A camera runs its outputs as one configured set, which Apple configures together (`AVCaptureSession` between `beginConfiguration` and `commitConfiguration`) and Android binds together (CameraX's `bindToLifecycle`, Camera2's capture session); what it can run is described up front, by Apple's `formats`, Android's stream configuration map and Windows' video profiles, so a program picks from what is there. Not outputs attached one by one, which hides a reconfiguration and fails late; not a request refused after the fact, as CameraX's throw and the web's `OverconstrainedError`. The outputs are a resource with setters, as `audio-context-options` is, so a later output adds a setter rather than breaking a record.
9. **A camera another program runs keeps its format: a later session runs in it and says so through `format()`**, as Windows' shared mode joins the format of the app in control; where the system runs one camera in two formats at once, `format()` is the program's pick. Media decision 8 stands: each program has a session of its own, and macOS shares the camera itself. Changing a shared camera's format comes later, as Windows' `SetFormatAsync` with its `FormatChanged`.
10. **A capture session tells its state: running, interrupted (resuming by itself) or ended,** as Apple's interruption and runtime error notices, the web's `mute` and `ended`, CameraX's camera state and Windows' `Failed` all separate the two. Cameras are listed with their position and their changes, as Apple's discovery session, Android's availability callback, the web's `devicechange` and Windows' `DeviceWatcher`; listing asks nothing, and opening asks `permissions.camera`.
11. **Photos, from media decision 4, come in a size from the format's list**, encoded by the system. **Recording moves to a turn of its own, later** (amending media decision 4), not to the codec family's: on every platform recording is the system's own output (`AVCaptureMovieFileOutput`, CameraX's `Recorder`, `MediaCapture`'s recording to a file, the web's `MediaRecorder`), a family apart from encoding frame by frame (`AVAssetWriter`, `MediaCodec`, Media Foundation's sink writer, WebCodecs). It adds a setter to `camera-outputs` and two methods to the session, breaking nothing, and it brings the microphone into the camera session (an audio input on Apple's session, CameraX's `withAudioEnabled`), which deserves its own design.

12. **A running camera keeps its program from freezing, as playing or recording sound does** (the lead's answer of 2026-11-03), inside the one freeze rule (`crates/waos/src/lifecycle/freeze_rule.rs`), from the experiment's suspend model (wasdk's `2026-09-26-suspend-model.md`): a video call in a hidden window keeps sending video, as on macOS. A phone's system interrupts a camera in the background itself, and the session says so. Not freezing interrupting the camera everywhere, the phone's model.
13. **`cameras.list` and `camera.start` are `async func`** (2026-11-03), so a program's call never waits on WaOS's main thread, where the system's cameras are driven: the web's `enumerateDevices` and `getUserMedia` answer promises, and Apple has a session started off the main queue, since `startRunning` blocks. Inset's `Cameras::cameras()` and `CameraDevice::start` answer futures to match, which WaOS's program host awaits.
14. **A camera says which way it faces, `camera-info.facing`** (2026-11-03), Inset's, Android's and the web's word, where Apple's is `position`. **An interruption's reason is optional** (2026-11-03): an enum, so a program words it in its own language, and none where the system gives none, as macOS's notice carries no reason. **Frame rates are `f64`**, as AVFoundation's ranges are, so a format crosses the WIT exactly.
15. **Sharing a camera's format needs no change to the interface** (2026-11-03): measured, a camera has one format for every session in a process; a session starting while another holds the device's lock leaves it alone; and the same process's own lock is granted again, so the format is kept by a joining session setting none of its own, as Inset's macOS host does. Windows' exclusive and shared modes, iOS's and Android's one user at a time and the web's per-track scaling all fit `start(format)` with `format()`.
16. **Inset's `VideoFrame` is a trait every host implements** (2026-11-03), amending native-surfaces decision 25's alias: a frame with its size and one conversion, `texture(device)`, the texture holding the frame, as the WIT's frame and hardware buffer have it, so `frame_for` answers a real frame on every host, WaOS's program host among them, and platform code reaches the system's own frame by downcasting to its host's frame type (`CoreVideoFrame` on Apple's systems). A reader's frame on a type only some hosts could make had left WaOS's program host answering none for ever.

17. **A player's position is read when it is needed, and its jumps are told** (2026-11-04), under the lead's standing principle that an interface shows what the platforms do and hides nothing for convenience (Butler Lampson's "Don't hide power"): `position()` answers where the player is now, as AVPlayer's `currentTime()`, Media3's `getCurrentPosition()`, Windows' `PlaybackSession.Position` and the web's `currentTime` do, and a change is told at each jump of the position (a seek landing, a loop: Apple's `AVPlayerItemTimeJumped`, Media3's `onPositionDiscontinuity`, the web's `seeked`), not on a timer. Not the position re-sent every half second in the player's state, which was Flutter's `video_player` polling, an interval no platform has.
18. **Cues come as the media styles them, and the user's caption style beside them** (2026-11-04), reversing decision 7's merge under the same principle: a player hands out the media's own cues (Apple's legible output at `SourceAndRulesOnly`, Media3's `Cue`s), and the system's caption style is a setting of its own, read and followed as the platform's brightness and locales are (Apple's `MACaptionAppearance`, Android's `CaptioningManager`, Windows' `ClosedCaptionProperties`; none on the web), each of its settings saying whether it applies always or only where the media says nothing, as Apple's do. The merge is the subtitle widget's, as Media3's `SubtitleView` applies its `CaptionStyleCompat` and Chromium's cue style sheet cascades, so a framework keeps the user's style without the interface hiding either fact; an app that edits or inspects captions gets both.

19. **The user's caption settings are a capability of their own, none where the system keeps none** (2026-11-04): in Inset, `Platform::caption_settings()` answers a `CaptionSettings` with the user's `style()` and a `changed()` that resolves at its next change, as a player's does, on macOS and iOS (`MACaptionAppearance`), Android (`CaptioningManager`) and Windows (`ClosedCaptionProperties`), and none on Linux and the web; the subtitle widget reads and follows it. Not a method on the platform beside a client callback whose default does nothing, relayed through the framework's binding to the one widget that cares.
20. **The framework follows the caption settings once, into the data every widget reads the system's settings from** (2026-11-04), amending decision 19's "the subtitle widget reads and follows it": Inset's binding follows `changed()` for as long as the app runs and tells its observers, `MediaQuery::from_view` takes the style again into `MediaQueryData::caption_style`, beside the text scale, bold text and brightness, and the subtitle widget reads it as it builds, as Flutter's widgets read those settings. Not each subtitle widget holding a task of its own on `changed()`, which a widget leaving the screen leaves waiting, and which no other widget, such as an app's own caption preview, could share.
21. **A player's state carries its clock, not its position** (2026-11-04), amending decision 17's `position()`: `player-state.timeline` is where the player was in the media at an instant and the rate it moves at from there, `0.0` while paused or waiting for media, which changes only when its progress breaks (a play, a pause, a change of rate, a wait for media and its end, a jump such as a seek landing or a loop), and from which a program works out the position at any instant itself. A program's player lives on WaOS's main thread, where Inset's players are, and the program runs on its own, so a position read when needed would have to be fetched; the systems keep a player's clock instead (Apple's item `timebase`, which any thread reads, with its notices of a change of rate and of a jump; Android's `MediaTimestamp` and its media time discontinuities), and a page reads a player that runs elsewhere by its clock: WebKit's from its GPU process through `TimeProgressEstimator`, Chromium's through `MojoRenderer`'s interpolator, each working the time out from the last one told, its moment and the rate. Inset's `PlayerState` carries the same `PlayerTimeline`. Not a `position()` the engine answers by waiting on the main thread at each frame, nor one it guesses from samples, which would run ahead while the player waits for media.
22. **A feature some platform never has is an optional part of the object that would carry it; a feature some media lack is a fact in the state** (2026-11-05), the rule of `get-compositor()` one level down: frame stepping, which Media3 and the web lack, would be a player's `frame-stepping: func() -> option<frame-stepping>`, never a method that does nothing; whether this stream can seek, or this item step backwards, is told by the state, as Apple's `canStepBackward`, Windows' `CanSeek` and the web's `seekable` tell it per item. A request whose promise every system keeps needs neither: a seek's tolerance is leave to land nearby ("within the range [time-toleranceBefore, time+toleranceAfter]", Apple's `AVPlayer.h`), which a system that always seeks exactly keeps too.
23. **Decision 21 is withdrawn, and decision 17's `position()` stands** (2026-11-05): a clock in the interface gave a program nothing a position and a rate do not, and the web has none; the clock was WaOS's thread split leaking out. WaOS's engine keeps one inside, from the position the main thread reads at each change, the time since and the rate while the player plays, and answers `position()` from it, as WebKit's page side answers `currentTime` for a player in its GPU process. The research behind the next decisions is `2026-11-04-player-research.md` (IINA, mpv, VLC, Media3).
24. **A player's state says what it is doing: paused, waiting or playing, each with a reason where the system gives one** (2026-11-05), as Apple's `timeControlStatus` and Windows' `PlaybackState` say it, replacing `playing`, which told what the program asked, and `ended`. Paused for: the program's request, the end (`ended` before), an interruption that took the sound (Apple's audio session interruption, Android's audio focus loss), the sound's output removed (Android's audio becoming noisy), the app sent to the background (Apple's). Waiting for: data, the system still judging whether data comes fast enough (Apple's, which advises showing no spinner yet), the system holding it back a moment (Android's short audio focus loss). A reason is optional, as decision 14's interruption reason is: none where the system says nothing, as the web never does. Not two flags, wanting to play and waiting, which allow combinations that mean nothing.
25. **A failure has a kind, can come at any time, and carries the system's own words for logs** (2026-11-05): network, decode, unsupported, protected, refused (the system or host does not let the player reach the source: WaOS's network rules or a path outside the program's files, Media3's permission and cleartext errors), other; the web's `MediaError` and Windows' `MediaPlayerError` share the first three, and Apple's `AVError` and Media3's error codes fall into them. A player that fails partway through, as Apple's failed-to-play-to-end tells, fails as one that never loaded does, and does nothing more; it is opened again to retry. Not a string, which a program can neither translate nor act on, nor the platforms' own codes, which a program would have to know for each.
26. **The state tells a seek under way, until it lands, and a seek says how near it must land** (2026-11-05): `seeking` is the target while a seek is under way, a later seek replacing it, as the web's `seeking`, mpv's seek and restart events, Media3's masked position, Apple's completion handler and Windows' `SeekCompleted` tell it; meanwhile the position reads the target, as the web's and Media3's do. `seek` takes a tolerance before and after the target, Apple's tolerances and Media3's `SeekParameters`, both zero for an exact seek and unbounded for the nearest keyframe; the web's `fastSeek` takes any tolerance but zero. A seek outside the seekable ranges moves nothing, as Apple cancels it. Not a seek answering when it lands, which only Apple has and which leaves the state silent.
27. **The state tells what is loaded and where the player can seek, as lists of time ranges** (2026-11-05): Apple's `loadedTimeRanges` and `seekableTimeRanges`, Windows' `GetBufferedRanges` and `GetSeekableRanges`, the web's `buffered` and `seekable`, mpv's cache ranges; Media3, which keeps data ahead only, gives one loaded range from the position, and its item's seekability. No seekable range means it cannot seek. Not one end each, which loses data kept behind the position, the gaps after a jump and where a live window starts.
28. **The state reads back the rate, the volume and whether the sound is muted, a switch of its own** (2026-11-05): the system changes them itself (Apple resets the rate at an interruption, Media3 nudges a live stream's speed, VLC warns the rate asked for may not be the one played), the rate is the one played at while playing, kept while paused (Apple's `defaultRate`, the web's `playbackRate`), and mute is apart from the volume on Apple, Windows, the web, mpv and VLC; Media3, which mutes by setting the volume to 0, has its host keep the volume to come back to.
29. **A caption setting is a record per kind of value: the value, and when it applies** (2026-11-05): `color-setting`, `number-setting`, `edge-setting`, `typeface-setting`, each a `value` and a `behavior`, `always` or `where-media-says-nothing`, as Inset's `CaptionSetting<T>` is one type over each, since WIT has no generic types. Not a behaviour field beside each value, which doubles the fields and lets a value and its behaviour be missing apart.
30. **The media interfaces are a WIT package of their own, `wasdk:media`, versioned apart from `wasdk:platform`, and the program's world a package above both, `wasdk:program`** (2026-11-05): the media players, the video frames, the cameras and the caption settings may break while the rest holds, and a host serves an old version beside the new on one implementation, the old translated where it changed (measured). The world moves out of `wasdk:platform`, since a world there importing `wasdk:media`, whose interfaces use `wasdk:platform`'s, is a cycle, which WIT refuses. The hardware buffers stay in `wasdk:platform`, a GPU buffer not only media's.
31. **A view holds the layers beneath its picture, set apart from the present, and its next present shows them with its picture** (2026-11-05): `view.set-layers(layers: list<placed-layer>)`, the layers kept until set again, and `view.present()` with no arguments, which wgpu's `Queue::present` calls through `wasdk-wgpu` (`2026-10-17-the-waos-project.md` decision 52), and a program without wgpu calls itself; a layer the list does not name is taken off with the next picture, and a present with no picture drawn since the last still does nothing, so a layer still moves with the picture that makes room for it. Every system keeps a compositor's layers as its own state, applied with the window's next frame: Wayland's synchronized subsurfaces with the parent's commit, which a Vulkan present makes; Android's `applyTransactionOnDraw`; Core Animation's `presentsWithTransaction`; the web's elements changed in the task that draws the canvas. This amends decision 1's layers named with each present. Not layers set on wgpu's frame, a wgpu type carrying WaOS's state, nor a present of the view's own beside wgpu's for a program with layers.
32. **A program reads media without playing it through `media-assets`, a sibling of the players: `get-media-assets()`, none where the system has none, `open(source)`, and an asset's `duration`, `tracks`, `metadata` and `frame-at(time, tolerance, max-size)`** (2026-11-06), the player's types reused (`media-track`, `media-metadata`, `seek-tolerance`, `player-error`, `video-frame`), for a library's thumbnails, lengths and titles before anything plays. Every system has this layer apart from its player and its codecs: Apple's `AVAsset` and `AVAssetImageGenerator`, Android's `MediaMetadataRetriever` and Media3's `FrameExtractor`, Windows' `MediaClip`. Not codecs, which decode frame by frame for editors and transcoders and would leave a program to read every file and stream format itself, which only the system can; nor a call on the player, which a library would have to open and play for each video.
33. **A program decodes and encodes images through the system's codecs, `image-codecs`, with GPU memory and the program's memory as two types and every move between them a call of its own** (2026-11-08): a `hardware-buffer`, as today, optional where a system has none, drawn without a copy, and gaining `read-pixels`, the one way back to the program's memory; a `pixel-buffer`, the program's own pixels, which it uploads with wgpu itself to draw; an `encoded-image` opened from encoded bytes, with its size, frames, rotation and metadata, decoded into a `pixel-buffer` on every system; and `hardware-image-codecs`, none where the system cannot, decoding into a `hardware-buffer` and encoding from one, as Apple decodes into IOSurfaces and Android's `ImageDecoder` allocates hardware bitmaps, and as Android keeps `Bitmap` and `HardwareBuffer` apart with `copy` between them. A video's thumbnail is its frame's hardware buffer encoded into the program's cache, and drawn again from it decoded. Not an opaque image whose memory is the system's choice, as Skia's `SkImage` and the web's `ImageBitmap` are, which would hide when pixels travel and what it costs; nor a program bundling its own decoders, which only reach the formats it carries, slower.
34. **A video's frame is a hardware buffer, and an image opened for decoding is an `image-source`** (2026-11-08): the `video-frame` resource, which only gave its `hardware-buffer`, goes, a frame reader's `frame-for` answering a `hardware-buffer` that holds its slot in the decoder's pool itself, and Waft's `VideoFrame`, which was only a hardware buffer's width, height, texture and pixels, becomes its `HardwareBuffer`; image codecs' `encoded-image` resource becomes `image-source`, as Apple's `CGImageSource` and Android's `ImageDecoder.Source` are named, leaving `encoded-image` to the bytes of an image as a value (a track's artwork, Waft's `EncodedImage`), and Waft's port of Flutter's `ui.Codec` keeps Flutter's names. A photo's location is read from its own metadata with no permission, the program holding the file's bytes already, and encoding writes no metadata. No name is kept for compatibility; nothing is released yet.
35. **A program has raw GPU memory and lent frames apart, as every system does: `hardware-buffer`, memory it can allocate, draw into, show on a composition layer and encode, and `video-frame`, a decoder's or a camera's frame lent with its slot in the system's pool** (2026-11-08): `get-hardware-buffers()`, none where the system has none, and `allocate(width, height, format, usage)`, as `IOSurfaceCreate`, `AHardwareBuffer_allocate` and a shared D3D texture; a buffer's `texture(device)`, to sample or to draw into as its usage allows, and `read-pixels`; a composition layer showing a buffer the program drew, as Core Animation's `contents` and Android's `ASurfaceTransaction_setBuffer`; a decoded image is a raw buffer, holding no pool slot, as Image I/O's and Android's hardware bitmaps; a video frame (`CVPixelBuffer`, `AImage`, WebCodecs' and WinRT's `VideoFrame`) gives its buffer, which keeps the frame alive while held, and a frame reader's `frame-for` answers frames again. Waft's `HardwareBuffer` and `VideoFrame` are these two already. This amends decision 34, which merged the two and hid the raw kind from programs. Not one kind with the lease hidden in it, which only the system's pools need; nor raw buffers kept from programs, which a video editor, a game of several layers and an exporter need.
36. **A hardware buffer knows nothing of the GPU API; `gpu-interop`, an interface of its own, imports it as a texture, and its usage is named after its users, as Android names them** (2026-11-08): `hardware-buffers` keeps a buffer's own facts (size, format, usage) and `read-pixels`, the CPU's access being the buffer's own (`IOSurfaceLock`, `AHardwareBuffer_lock`), and names `wasi:webgpu` nowhere; `gpu-interop.import-texture(device, buffer)` shares its memory with the program's device, as on every system the GPU side imports (Metal's `newTextureWithDescriptor:iosurface:`, Vulkan's external memory, D3D's `OpenSharedResource`, WebGPU's `importExternalTexture`), Core Video's texture cache being such a bridge of its own; the interface is named as Vulkan, CUDA and Android name sharing with the GPU API, interop, so a fence or a semaphore shared later belongs there too. In Rust `wasdk-wgpu`, the GPU side, offers it with wgpu's types (`wasdk_wgpu::import_texture`), and Waft's `Platform::import_texture` takes it from Waft's `HardwareBuffer` and `VideoFrame`, the host being the one that knows both. A buffer's usage flags are `gpu-sampled` and `gpu-color-output`, Android's `GPU_SAMPLED_IMAGE` and `GPU_COLOR_OUTPUT`, the CPU, the compositor and a video encoder to join them as they are needed, rather than a texture's usages in wgpu's words. Not the import on the buffer, which bound the buffer to one GPU API; nor `gpu-imports`, narrower than the role.
37. **What makes a program's hardware buffers is the hardware buffer allocator** (2026-11-08): `get-hardware-buffer-allocator() -> option<hardware-buffer-allocator>`, with `allocate`, and Waft's `Platform::hardware_buffer_allocator()` and `HardwareBufferAllocator`, as Android's graphics allocator (gralloc's `IAllocator`), Fuchsia's sysmem `Allocator` and Linux's GBM device name the role; not `hardware-buffers`, a plural that reads as a list of buffers. The other plural services (`media-players`, `media-assets`, `image-codecs`, `video-frame-readers`, `cameras`) are a question of their own, left as they are until it is taken.
38. **An image says how its pictures go: `pictures`, one still picture, an animation's frames played a number of times or forever, or a document's pages with its primary page** (2026-11-16): `image-source.pictures() -> image-pictures`, a variant of `still`, `animation` (frames with their durations, and `plays`, `times(n)` or `forever`, counted in plays as Image I/O, APNG and WebP count them) and `pages` (each page's size and orientation, and the index of the primary one); `size`, `orientation` and `metadata` are the primary picture's, and the decode calls take a picture's `index`. Every image of several frames otherwise played forever, a TIFF's pages included. Waft shows a document's primary page, as Flutter shows a file's first image; a program reaches the other pages through the codecs. Measured: an animation is a file-level `LoopCount` in its format's dictionary, counting plays, 0 meaning forever, and several images without one are a document's pages. This amends decision 33's frames.

## The interface

```wit
interface views {
  // … unchanged but for what follows.
  use windows.{rect, size};

  /// A layer of the system's compositor that the program places beneath its drawing, where it
  /// leaves it clear: a player's video, a camera's preview. Made on its own; shown while a
  /// present names it.
  resource composition-layer {}

  /// The system's compositor, which makes the layers a program places beneath its drawing.
  resource compositor {
    /// A layer placed nowhere yet, `opaque` when nothing beneath it will show through it.
    make-layer: func(opaque: bool) -> composition-layer;
  }

  /// The system's compositor, where it has one.
  get-compositor: func() -> option<compositor>;

  record placed-layer {
    layer: borrow<composition-layer>,
    placement: placement,
  }

  /// Where a layer sits: its rectangle, then what the program's containers above it do to it,
  /// the outermost first, in the coordinates the transforms before each lead to.
  record placement {
    rect: rect,
    mutators: list<mutator>,
  }

  variant mutator {
    /// A 4×4 matrix, column-major.
    transform(list<f32>),
    clip-rect(rect),
    clip-rounded-rect(rounded-rect),
    /// The alpha it shows at, out of 255.
    opacity(u8),
  }

  /// A rectangle with each corner's radii, horizontal and vertical.
  record rounded-rect {
    rect: rect,
    top-left: size,
    top-right: size,
    bottom-right: size,
    bottom-left: size,
  }

  resource view {
    /// Puts the texture last taken from the context on screen, with these layers beneath it.
    /// A layer this present does not name is taken off. A present with no picture drawn since
    /// the last does nothing, its layers included, since a layer moves with the picture that
    /// makes room for it.
    present: func(layers: list<placed-layer>);
  }

  record frame {
    /// When the screen's refresh this frame is for began: what animations advance to.
    time: instant,
    /// When this frame's picture is expected to reach the screen: what a video frame is asked
    /// for.
    display-time: instant,
    views: list<view-id>,
  }
}

interface hardware-buffers {
  use wasi:webgpu/webgpu@0.3.0-rc.2.{gpu-device, gpu-texture};

  /// Pixels that the GPU and the system's compositor both reach: an `IOSurface`, an
  /// `AHardwareBuffer`; on the web, a `VideoFrame`.
  resource hardware-buffer {
    width: func() -> u32;
    height: func() -> u32;
    /// A texture of the buffer on `device`, for sampling. Native hosts share its memory; a web
    /// host copies it.
    texture: func(device: borrow<gpu-device>) -> gpu-texture;
  }
}

interface video-frames {
  use hardware-buffers.{hardware-buffer};
  use views.{composition-layer, instant};

  /// One picture of a video. It holds its slot in the decoder's pool until it, its buffer and
  /// their textures are dropped.
  resource video-frame {
    hardware-buffer: func() -> hardware-buffer;
  }

  /// The system's frame readers, where it hands out frames.
  resource video-frame-readers {
    /// A reader fed by nothing yet.
    make-reader: func() -> video-frame-reader;
  }

  get-video-frame-readers: func() -> option<video-frame-readers>;

  /// Where a player or a camera sends its frames for the program to draw; given to one source
  /// at a time.
  resource video-frame-reader {
    /// The frame to show at `display-time`: a player's frame for that moment, a camera's
    /// newest. None when it is the frame answered last, or before the first.
    frame-for: func(display-time: instant) -> option<video-frame>;
  }

  /// Where a source's picture goes.
  variant video-output {
    /// The system draws the video on this layer.
    layer(borrow<composition-layer>),
    /// The program draws the frames this reader reads.
    frames(borrow<video-frame-reader>),
  }
}

interface media-players {
  use wasi:clocks/types@0.3.0.{duration};
  use windows.{size};
  use video-frames.{video-output};

  resource media-players {
    /// A player of `source`: a path the program can read, or a link, which passes the
    /// program's network rules first. It loads by itself; its state tells how far it got.
    open: func(source: string) -> media-player;
  }

  get-media-players: func() -> option<media-players>;

  resource media-player {
    /// Where the picture goes, one destination at a time; none for no picture.
    set-video-output: func(output: option<video-output>);
    play: func();
    pause: func();
    /// Moves to `position`; the position it reaches shows in its state, as Inset, Media3,
    /// Windows and the web seek with no answer.
    seek: func(position: duration);
    set-rate: func(rate: f64);
    set-volume: func(volume: f32);
    set-looping: func(looping: bool);
    state: func() -> player-state;
    state-changes: func() -> stream<player-state>;
    /// Plays the audio track `id` in place of the one playing.
    select-audio-track: func(id: string);
    /// Turns the text track `id` on, or none: subtitles off. Its cues arrive on `cue-changes`.
    select-text-track: func(id: option<string>);
    /// The cues to show now, again each time they change; empty while no text track is on.
    cue-changes: func() -> stream<list<cue>>;
    /// What the media says about itself; empty until it is ready.
    metadata: func() -> media-metadata;
    /// The metadata again each time it changes, such as a live stream's current title.
    metadata-changes: func() -> stream<media-metadata>;
  }

  record player-state {
    status: player-status,
    /// Whether it plays, or waits for media to play on: from `play` until `pause`, or the end
    /// of media it does not loop.
    playing: bool,
    position: duration,
    /// None until known, and for a live stream.
    duration: option<duration>,
    /// The picture's size as it is presented; none until known, and for media without one.
    size: option<size>,
    /// The media's tracks; empty until it is ready.
    tracks: list<media-track>,
    /// Whether it reached the end of media it does not loop, until it plays or seeks again.
    ended: bool,
  }

  variant player-status {
    loading,
    ready,
    /// It cannot play, for the system's reason.
    failed(string),
  }

  record media-track {
    /// Names the track for this player's run.
    id: string,
    kind: media-track-kind,
    /// A BCP 47 language tag; empty where the media gives none.
    language: string,
    /// Its name as the media gives it; empty where it gives none.
    label: string,
    /// Whether it plays now: one audio track at a time, and one text track or none.
    selected: bool,
  }

  enum media-track-kind {
    audio,
    video,
    text,
  }

  /// What the media says about itself, from its tags; a field is none where it does not say.
  record media-metadata {
    title: option<string>,
    artist: option<string>,
    album: option<string>,
    /// The cover image, encoded as the media holds it.
    artwork: option<encoded-image>,
  }

  record encoded-image {
    mime-type: string,
    data: list<u8>,
  }

  /// One subtitle or caption to show: Media3's `Cue`, field for field, with the user's caption
  /// settings merged in.
  record cue {
    content: cue-content,
    /// Where its box sits from the top: a fraction of the picture's height, or a line number;
    /// none for the default, the bottom.
    line: option<cue-line>,
    line-anchor: cue-anchor,
    /// Where its box sits across, as a fraction of the picture's width; none for the default,
    /// the centre.
    position: option<f32>,
    position-anchor: cue-anchor,
    /// Its box's width as a fraction of the picture's; none to fit what it holds.
    size: option<f32>,
    text-alignment: cue-text-alignment,
    vertical: option<cue-vertical>,
    /// The colour of the box behind it; none for no box.
    window-color: option<color>,
  }

  variant cue-content {
    /// Its text, in runs of one style.
    text(list<cue-text-run>),
    /// A picture subtitle, as Blu-ray, DVD and DVB carry them, decoded.
    image(cue-image),
  }

  record cue-text-run {
    text: string,
    bold: bool,
    italic: bool,
    underline: bool,
    color: color,
    /// The colour behind these characters; none for none.
    background: option<color>,
    /// The text's height as a fraction of the picture's.
    font-size: f32,
    /// None for the system's caption font.
    font-family: option<string>,
    edge: cue-text-edge,
    edge-color: color,
  }

  record cue-image {
    width: u32,
    height: u32,
    /// RGBA, 8 bits a channel, rows from the top.
    pixels: list<u8>,
    /// Its height on screen as a fraction of the picture's; its width is the cue's `size`.
    display-height: f32,
  }

  variant cue-line {
    fraction(f32),
    number(s32),
  }

  enum cue-anchor {
    start,
    middle,
    end,
  }

  enum cue-text-alignment {
    start,
    center,
    end,
  }

  enum cue-vertical {
    right-to-left,
    left-to-right,
  }

  enum cue-text-edge {
    none,
    outline,
    drop-shadow,
    raised,
    depressed,
  }

  /// 0xAARRGGBB.
  type color = u32;
}

interface cameras {
  use windows.{size};
  use views.{composition-layer};
  use video-frames.{video-frame-reader};

  resource cameras {
    /// The cameras now; asks nothing.
    %list: async func() -> list<camera-info>;
    /// The list again each time a camera comes or goes.
    changes: func() -> stream<list<camera-info>>;
    /// Asks `permissions.camera` at the first use.
    open: async func(id: string) -> result<camera, camera-error>;
  }

  get-cameras: func() -> option<cameras>;

  record camera-info {
    id: string,
    name: string,
    facing: camera-facing,
  }

  enum camera-facing {
    front,
    back,
    external,
  }

  /// One way this camera runs, with all its outputs.
  record camera-format {
    /// Its frames' and preview's size.
    size: size,
    min-frame-rate: f64,
    max-frame-rate: f64,
    /// The sizes it takes photos at while running so.
    photo-sizes: list<size>,
  }

  /// What a camera feeds, given whole to `start`.
  resource camera-outputs {
    constructor();
    /// The system draws the preview on this layer.
    set-preview: func(layer: borrow<composition-layer>);
    /// Frames for the program, read by this reader.
    set-frames: func(reader: borrow<video-frame-reader>);
    /// Photos at this size, one of the format's `photo-sizes`, taken with `take-photo`.
    set-photos: func(size: size);
  }

  resource camera {
    /// The formats this camera runs in.
    formats: func() -> list<camera-format>;
    /// Starts the camera in `format`, or joins it in the format it already runs for another
    /// program. An error only for what can happen at any time: the camera is in use,
    /// unplugged, or not allowed.
    start: async func(format: camera-format, outputs: borrow<camera-outputs>) -> result<capture-session, camera-error>;
  }

  /// The camera running for this program; dropping it stops the camera for this program.
  resource capture-session {
    /// The one picked, unless this session joined another program's.
    format: func() -> camera-format;
    state: func() -> capture-state;
    state-changes: func() -> stream<capture-state>;
    /// The photo, encoded by the system.
    take-photo: async func() -> result<photo, camera-error>;
  }

  variant capture-state {
    running,
    /// No frames for now; it resumes by itself. The reason is none where the system gives
    /// none.
    interrupted(option<camera-interruption>),
    /// For good.
    ended(camera-error),
  }

  enum camera-interruption {
    /// Another app has the camera, as a phone allows one at a time.
    in-use-by-another-app,
    /// The program is in the background.
    background,
    /// The system is too hot or too busy.
    system-pressure,
  }

  variant camera-error {
    not-allowed,
    in-use,
    unplugged,
    failed(string),
  }

  record photo {
    mime-type: string,
    data: list<u8>,
  }
}
```

## What changes in Inset

Inset's host traits follow the same shapes, so that WaOS passes them through:

- A `VideoFrameReader` made on its own, as a composition layer is, given to a player's `set_video_output` or among a capture session's outputs, in place of `read_frames()` and `frame_output()`; this closes native-surfaces gap 31's readers that behave differently.
- `MediaPlayer` gains `PlayerState.tracks`, `select_audio_track`, `select_text_track`, `metadata` and `cues`, the user's caption settings merged in by the host; on macOS from `AVPlayerItem`'s media selection groups, `commonMetadata`, and an `AVPlayerItemLegibleOutput` with `suppressesPlayerRendering`.
- A camera lists its formats and starts with its outputs whole, its session telling its format and an interrupted or ended state.
- The widgets gain a subtitle widget drawing a player's cues, which `PlayerView` puts over the video.

## Later, each only adding to the interface

- Recording, with the microphone in the camera session (decision 11).
- The codec family: decoding and encoding frame by frame, as WebCodecs, `AVAssetReader` and `MediaCodec` (native-surfaces decision 27).
- YUV planes and colour for frames a program wants unconverted; a frame's time, rotation and colour (native-surfaces gap 28).
- Changing a shared camera's format, as Windows' `SetFormatAsync`.
- Choosing a stream's quality (Apple's `preferredMaximumResolution`, Media3's `setMaxVideoSize`).
- The host telling Apple's accessibility which captions are shown (`MACaptionAppearanceDidDisplayCaptions`).
