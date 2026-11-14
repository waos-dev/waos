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

use crate::os::FrameMemory;
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
}
