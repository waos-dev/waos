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

use tokio::sync::OwnedSemaphorePermit;

use crate::bindings::wasdk::media::video_frames::{
    ColorRange, HdrMetadata, PixelAspect, PixelRect, VideoColor, VideoPlane,
};
use crate::os::{self, FrameMemory};
use crate::shell::{DecodedFrame, ShellVideoFrameReader, ShellVideoFrameReaders, VideoFrame};

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

/// `video-frames.video-frame`: one frame a reader answered or a decoder put out, the system's
/// own, with how it is shown and what its source says of it.
pub struct ProgramVideoFrame {
    lent: LentFrame,
    rotation: u32,
    flip: bool,
    /// What its decoder said of it; none for a reader's frame, which says only what it is.
    decoded: Option<DecodedFacts>,
}

/// What a decoder says of a picture it put out.
struct DecodedFacts {
    timestamp: i64,
    duration: Option<u64>,
    visible_rect: PixelRect,
    pixel_aspect: PixelAspect,
    color: VideoColor,
    hdr_metadata: Option<HdrMetadata>,
}

/// The system's frame as whatever shows or reads it holds it: its memory, with its slot among the
/// frames its decoder lends at once, where it has one. Every clone holds both, so the slot is free
/// again only once the frame, its hardware buffer and every texture imported of it are dropped.
#[derive(Clone)]
pub(crate) struct LentFrame {
    memory: FrameMemory,
    _slot: Option<Arc<OwnedSemaphorePermit>>,
}

impl LentFrame {
    pub(crate) fn memory(&self) -> &FrameMemory {
        &self.memory
    }
}

impl ProgramVideoFrame {
    /// `frame`, the system's, as the program holds a reader's.
    pub(crate) fn of(frame: VideoFrame) -> ProgramVideoFrame {
        ProgramVideoFrame {
            lent: LentFrame {
                memory: FrameMemory::new(frame.picture),
                _slot: None,
            },
            rotation: frame.rotation,
            flip: frame.flip,
            decoded: None,
        }
    }

    /// `decoded`, a decoder's, as the program holds it, in `slot` of the frames the decoder
    /// lends at once.
    pub(crate) fn of_decoder(
        decoded: DecodedFrame,
        slot: OwnedSemaphorePermit,
    ) -> ProgramVideoFrame {
        ProgramVideoFrame {
            lent: LentFrame {
                memory: FrameMemory::new(decoded.frame.picture),
                _slot: Some(Arc::new(slot)),
            },
            rotation: decoded.frame.rotation,
            flip: decoded.frame.flip,
            decoded: Some(DecodedFacts {
                timestamp: decoded.timestamp,
                duration: decoded.duration,
                visible_rect: decoded.visible_rect,
                pixel_aspect: decoded.pixel_aspect,
                color: decoded.color,
                hdr_metadata: decoded.hdr_metadata,
            }),
        }
    }

    /// The frame as its hardware buffer holds it too.
    pub(crate) fn lent(&self) -> &LentFrame {
        &self.lent
    }

    /// The turn applied when it is shown, in degrees clockwise, before `flip`.
    pub(crate) fn rotation(&self) -> u32 {
        self.rotation
    }

    /// Whether it is shown flipped horizontally, after `rotation`.
    pub(crate) fn flip(&self) -> bool {
        self.flip
    }

    /// When it is shown, on its stream's own timeline; none for a reader's frame, which comes
    /// for a display time and carries no time of its own.
    pub(crate) fn timestamp(&self) -> Option<i64> {
        self.decoded.as_ref().map(|decoded| decoded.timestamp)
    }

    pub(crate) fn duration(&self) -> Option<u64> {
        self.decoded.as_ref().and_then(|decoded| decoded.duration)
    }

    /// The picture's planes, as a texture is imported of each: one of red, green and blue for a
    /// reader's frame, a decoder's own two where the system hands them out, and none for a
    /// layout the engine makes no textures of. A failure is the shell's, which handed out a frame
    /// of a kind the engine does not take.
    pub(crate) fn planes(&self) -> anyhow::Result<Option<Vec<VideoPlane>>> {
        let buffer = self.lent.memory.hardware_buffer()?;
        Ok(os::planes(&buffer))
    }

    /// The part of the buffer that is picture: what its decoder says, and all of a reader's
    /// frame.
    pub(crate) fn visible_rect(&self) -> anyhow::Result<PixelRect> {
        if let Some(decoded) = &self.decoded {
            return Ok(decoded.visible_rect);
        }
        let buffer = self.lent.memory.hardware_buffer()?;
        Ok(PixelRect {
            x: 0,
            y: 0,
            width: buffer.width(),
            height: buffer.height(),
        })
    }

    /// The shape of one pixel: what its decoder says, and square for a reader's frame, which is
    /// drawn for showing as it is.
    pub(crate) fn pixel_aspect(&self) -> PixelAspect {
        match &self.decoded {
            Some(decoded) => decoded.pixel_aspect,
            None => PixelAspect {
                width: 1,
                height: 1,
            },
        }
    }

    /// What the samples mean as colour: what its decoder says. A reader's frame is red, green
    /// and blue over its whole range, which is all the system says of it: its primaries and how
    /// it encodes brightness are its video's, and the shell is not told them.
    pub(crate) fn color(&self) -> VideoColor {
        match &self.decoded {
            Some(decoded) => decoded.color,
            None => VideoColor {
                primaries: None,
                transfer: None,
                matrix: Some(RGB_MATRIX),
                range: Some(ColorRange::Full),
                chroma_location: None,
            },
        }
    }

    /// The brightness it was mastered for, where its decoder says; the shell is not told a
    /// reader's.
    pub(crate) fn hdr_metadata(&self) -> Option<HdrMetadata> {
        self.decoded
            .as_ref()
            .and_then(|decoded| decoded.hdr_metadata)
    }
}

/// H.273's matrix code for samples that are red, green and blue already.
const RGB_MATRIX: u8 = 0;
