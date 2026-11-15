//! The `tracing` targets the embedder records its work under, so a subscriber of the program's
//! own picks what to read.

/// Each frame of a video the embedder takes from a reader to draw, at `trace`, one event each,
/// with the display time it was asked for in its `display_time` field.
pub const VIDEO_FRAMES: &str = "wasdk_adapter_waft::video_frames";
