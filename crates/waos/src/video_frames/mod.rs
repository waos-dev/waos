//! `wasdk:media/video-frames`: a video's frames for the program to draw. The shell's readers make
//! each reader ([`ShellVideoFrameReaders`]), which a program gives a player or a camera as its
//! picture's destination and asks for the frame of a display time ([`ShellVideoFrameReader`]); a
//! frame the reader answers is the system's own, lent to the program as a [`ProgramVideoFrame`],
//! whose hardware buffer it samples through a texture it imports (`hardware_buffers`,
//! `gpu_interop`). The interface is served to a program by `host`.
//!
//! The invariant: a frame, its hardware buffer and the textures imported of it each hold the
//! system's frame, so its decoder does not hand the frame's pixels out again while any of them is
//! held (decisions 3 and 35 of `docs/proposals/2026-11-02-program-media.md`).

pub(crate) mod host;

use std::sync::Arc;

use crate::bindings::wasdk::media::video_frames::{
    ColorRange, PixelRect, PlaneContent, PlaneFormat, VideoColor, VideoPlane,
};
use crate::gpu::wgpu_types;
use crate::os::{self, FrameMemory};
use crate::shell::{ShellVideoFrameReader, ShellVideoFrameReaders, VideoFrame};

/// `video-frames.video-frame-readers`: the shell's readers, as the program reaches them.
pub struct VideoFrameReaders(Arc<dyn ShellVideoFrameReaders>);

/// `video-frames.video-frame-reader`: a reader the shell's readers made for the program, which a
/// player it is given to hands back to the shell.
pub struct VideoFrameReader(Arc<dyn ShellVideoFrameReader>);

impl VideoFrameReader {
    /// The reader the shell made, which a player is given to send its frames to.
    pub(crate) fn shell_reader(&self) -> &Arc<dyn ShellVideoFrameReader> {
        &self.0
    }
}

/// `video-frames.video-frame`: one frame a reader answered, the system's own, with how it is
/// shown.
pub struct ProgramVideoFrame {
    memory: FrameMemory,
    rotation: u32,
    flip: bool,
}

impl ProgramVideoFrame {
    /// `frame`, the system's, as the program holds it.
    pub(crate) fn of(frame: VideoFrame) -> ProgramVideoFrame {
        ProgramVideoFrame {
            memory: FrameMemory::new(frame.picture),
            rotation: frame.rotation,
            flip: frame.flip,
        }
    }

    /// The frame's memory, which its hardware buffer holds too.
    pub(crate) fn memory(&self) -> &FrameMemory {
        &self.memory
    }

    /// The turn applied when it is shown, in degrees clockwise, before `flip`.
    pub(crate) fn rotation(&self) -> u32 {
        self.rotation
    }

    /// Whether it is shown flipped horizontally, after `rotation`.
    pub(crate) fn flip(&self) -> bool {
        self.flip
    }

    /// The picture's planes. A reader's frame is red, green and blue in one plane, the whole of
    /// its buffer, as its player or camera was asked for it. A failure is the shell's, which
    /// handed out a frame of a kind the engine does not take.
    pub(crate) fn planes(&self) -> anyhow::Result<Vec<VideoPlane>> {
        let buffer = self.memory.hardware_buffer()?;
        Ok(vec![VideoPlane {
            width: buffer.width(),
            height: buffer.height(),
            format: rgb_plane_format(),
            content: PlaneContent::Rgba,
            bit_depth: 8,
        }])
    }

    /// The part of the buffer that is picture: all of a reader's frame.
    pub(crate) fn visible_rect(&self) -> anyhow::Result<PixelRect> {
        let buffer = self.memory.hardware_buffer()?;
        Ok(PixelRect {
            x: 0,
            y: 0,
            width: buffer.width(),
            height: buffer.height(),
        })
    }

    /// What the samples mean as colour. A reader's frame is red, green and blue over its whole
    /// range, which is all the system says of it: its primaries and how it encodes brightness
    /// are its video's, and the shell is not told them.
    pub(crate) fn color(&self) -> VideoColor {
        VideoColor {
            primaries: None,
            transfer: None,
            matrix: Some(RGB_MATRIX),
            range: Some(ColorRange::Full),
            chroma_location: None,
        }
    }
}

/// H.273's matrix code for samples that are red, green and blue already.
const RGB_MATRIX: u8 = 0;

/// The plane format of the system's hardware buffers, which a reader's frame is in.
fn rgb_plane_format() -> PlaneFormat {
    match os::FORMAT {
        wgpu_types::TextureFormat::Rgba8Unorm => PlaneFormat::Rgba8,
        _ => PlaneFormat::Bgra8,
    }
}
