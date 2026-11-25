# Research for the video codecs interface

What each system's video decoders and encoders take and give, read from their own sources on 2026-11-24, as the evidence the [draft interface](../../draft/video-codecs/README.md) was settled on.

- [decoders.md](decoders.md): naming a codec, how a stream is framed, what a decoded frame carries, flow and failure, what a system can say before trying, and where WebCodecs is limiting.
- [decoded-frames.md](decoded-frames.md): what a GPU can read of a decoded picture on each system, Android's layouts no app reaches, and what wgpu 30 and Dawn do with them.
- [encoders.md](encoders.md): what an encoder is told and what changes while it runs, its input and output, rate control, and what its system lists of it.

A research agent wrote them. Each claim is marked **verified**, read in the source that day, or **from memory**; each report's head lists what it read and what it did not. Twenty-three of the verified claims the design rests on most were checked again against Chromium, wgpu 30.0.2, the Android NDK and Apple's SDK headers, and held. A claim marked from memory is to be checked before anything is built on it.
