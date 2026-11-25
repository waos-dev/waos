# Video codecs (draft)

A draft of the interface for the system's video decoders and encoders, settled with the lead on 2026-11-24. The decoding half is in `wit/` as version 2.2.0, which is the contract now; the encoding half goes there when an encoder is built. Until then these files are its design, and what building finds wrong is changed here first.

- [video-streams.wit](video-streams.wit): what the two halves share: the codec, how a stream is framed and set up, one encoded picture, hardware or software.
- [video-decoders.wit](video-decoders.wit): the decoder.
- [video-encoders.wit](video-encoders.wit): the encoder.
- [video-frames.diff](video-frames.diff): what `video-frame` gains: its time, visible rectangle, planes, colour and HDR values, `read-planes`, and `make-frame`.
- [gpu-interop.diff](gpu-interop.diff): `import-plane`, a texture over one plane of a frame.

The three `.wit` files parse beside the rest of the package, and the two patches are against version 2.1.0, as it was before the decoding half was written. The `@since` markers and the versions (2.2.0) are added when a part is written to `wit/`.

## What was decided, and why

Each follows what the systems' own codecs take and give, so an app is not blocked later by a shape copied from the web that a system does not fit.

- **Chunks in, frames out on a stream.** Every system's decoder is queued this way.
- **A stream is its codec and its setup bytes, with no codec string.** Every decoder is configured from the bytes and reads the profile out of them; WebCodecs says the profile twice, and its own implementation has an open question on which wins.
- **H.264 and HEVC say how they are framed.** Files hold lengths, streams start codes; each system takes one, and the engine converts. The web infers the framing from whether setup bytes are present, and Chromium's source says of the same inference that it should be said outright.
- **The app says `any`, `hardware` or `software`, and the codec says which it is.** Apple can require hardware and Android lists each codec's kind; the web has only a hint.
- **Errors are named by what the app can do.** A damaged stream leaves the decoder alive, so a call that loses a packet does not rebuild a hardware decoder.
- **`decode` and `encode` wait while the codec is full.** Systems hand out input slots; a counter the app must watch, as the web has, is easy to ignore.
- **The app states how many frames it holds at once.** Decoded frames are lent from a pool three of the four systems size when the decoder is made.
- **`drain` and `reset`.** The web's `flush` means the opposite on Android.
- **The same decoder takes a new setup.** Apple can ask a session whether it accepts a new format, and Android changes size in place when told the largest beforehand.
- **"Can it decode?" answers yes, no or unknown, for hardware and for software.** Apple tells only whether a codec has hardware at all; a guess presented as a fact is how the web came to say "supported" and then decode in software.
- **Frames come out in the stream's own order, and a chunk's time is a label.** Every system's decoder but Apple's orders pictures from the stream; VideoToolbox leaves it to its caller, so there the order is read from the stream for it. An app whose times are missing or wrong gets the same order everywhere.
- **A chunk is its bytes, timestamp, duration and key flag.** No system's decoder takes an alpha side stream; an app runs two decoders and joins the planes itself.
- **A purpose, not a latency flag.** Apple and Android tell three uses apart: live, played, and unwatched.
- **A frame is the decoder's own picture.** Nothing is converted to RGB behind the app. It reads the planes where the system has them; where it keeps the layout to itself, as many Android decoders do, the frame has none and is shown on a composition layer.
- **One frame type for players, cameras and decoders,** as `CVPixelBuffer`, `AImage` and the web's `VideoFrame` are. An RGB picture is a frame with one plane.
- **The stream's colour wins, and the app's fills in what it leaves unsaid,** as Chromium's decoder and FFmpeg do.
- **A layout is its planes, not a named format,** so a layout no one named yet needs no new version.
- **A frame's planes are copied into the app's memory as they are,** for an app that works on a picture with the CPU, as a software renderer does. Every system gives the planes this way (a lock on Apple and Android, a map on Linux and Windows, `copyTo` on the web); a picture converted to RGB is always made from them.
- **Rate control is a kind with its own numbers.** The kinds cannot be mixed and each system has its own set; the web's variable rate has no peak.
- **`encode` takes any frame.** On Android the engine draws it into the encoder's surface, one pass on the GPU.
- **The app picks the encoder's framing, and the setup comes out as the type a decoder is opened with.**
- **The system's encoders are listed.** Every system lists them, and a recorder shows its user the list.

## Left out, to add as functions when an app needs them

- **Reading a frame whole,** as WebGPU's external texture: the one way to draw a frame with no planes. It needs an external texture in the WebGPU interface and a YCbCr sampler in wgpu's Vulkan backend. Until then an app on Android shows decoded frames on a layer and cannot draw them itself.
- **A copy to RGB.** Apple has a call for it and Android has none, so the right shape is not settled. An app mixes the planes it reads itself.
- **A target from the encoder to draw into,** as Android's input surface and Apple's pixel buffer pool are.
- **Side data on a chunk, dynamic HDR data, temporal layers and reference control, protected content, audio codecs.**
