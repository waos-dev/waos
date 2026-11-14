//! Waft's frame readers over `wasdk:media/video-frames`, where the system hands out frames: a
//! reader the app makes is the WIT's, which a player or a camera is given as its picture's
//! destination (`players`, `cameras`), and which answers Waft's `frame_for` with the WIT reader's
//! frame for that moment on the program's clock. A frame is the WIT's, a reader's or a media
//! asset's picture, lent with its slot in the decoder's pool, with its hardware buffer
//! (`hardware_buffers`), which the views import as a texture on the program's device
//! (`gpu_interop`), as they draw any reader's frames (`shown_videos`), and whose pixels are the
//! buffer's `read-pixels`.
//!
//! The invariant: only a reader these readers made reaches the WIT as a player's or a camera's
//! destination, by the handle the program holds; one the app got elsewhere sends their picture
//! nowhere. A frame holds its place in the decoder's pool until it, its buffer and the textures
//! imported of it are dropped.

use std::any::Any;
use std::sync::Arc;
use std::time::Instant;

use waft_embedder::image_codecs::{ImageFuture, PixelBuffer};
use waft_embedder::{HardwareBuffer, VideoFrame, VideoFrameReader, VideoFrameReaders};
use wasdk::video_frames as platform;

use crate::hardware_buffers::framework_buffer;
use crate::moments::wit_instant;

/// The system's frame readers.
pub(crate) struct WasdkVideoFrameReaders(platform::VideoFrameReaders);

impl WasdkVideoFrameReaders {
    /// The system's readers, where it hands out frames.
    pub(crate) fn get() -> Option<WasdkVideoFrameReaders> {
        platform::get_video_frame_readers().map(WasdkVideoFrameReaders)
    }
}

impl VideoFrameReaders for WasdkVideoFrameReaders {
    fn make_reader(&self) -> Arc<dyn VideoFrameReader> {
        Arc::new(WasdkReader(self.0.make_reader()))
    }
}

/// A reader [`WasdkVideoFrameReaders`] made: the WIT's.
struct WasdkReader(platform::VideoFrameReader);

impl VideoFrameReader for WasdkReader {
    /// The WIT reader's frame for `display_time`, a moment on the program's clock.
    fn frame_for(&self, display_time: Instant) -> Option<Arc<dyn VideoFrame>> {
        let frame = self.0.frame_for(wit_instant(display_time))?;
        Some(framework_frame(frame))
    }
}

/// The WIT's `frame` as Waft's, with its hardware buffer and how it is shown: a reader's, or a
/// media asset's picture.
pub(crate) fn framework_frame(frame: platform::VideoFrame) -> Arc<dyn VideoFrame> {
    let buffer = framework_buffer(frame.hardware_buffer());
    Arc::new(WasdkFrame {
        rotation: frame.rotation(),
        flip: frame.flip(),
        _frame: frame,
        buffer,
    })
}

/// A frame the WIT lent, with its hardware buffer, which holds it too, and which its size and
/// pixels are, and the turn and mirror it is shown with.
struct WasdkFrame {
    _frame: platform::VideoFrame,
    buffer: Arc<dyn HardwareBuffer>,
    rotation: u32,
    flip: bool,
}

impl VideoFrame for WasdkFrame {
    fn width(&self) -> u32 {
        self.buffer.width()
    }

    fn height(&self) -> u32 {
        self.buffer.height()
    }

    fn read_pixels(&self) -> ImageFuture<PixelBuffer> {
        self.buffer.read_pixels()
    }

    fn hardware_buffer(&self) -> Arc<dyn HardwareBuffer> {
        Arc::clone(&self.buffer)
    }

    fn rotation(&self) -> u32 {
        self.rotation
    }

    fn flip(&self) -> bool {
        self.flip
    }
}

/// The WIT's reader behind `reader`, which a player or a camera is given; none for a reader these
/// readers did not make.
pub(crate) fn wit_reader(
    reader: &Arc<dyn VideoFrameReader>,
) -> Option<&platform::VideoFrameReader> {
    let any: &dyn Any = &**reader;
    any.downcast_ref::<WasdkReader>().map(|reader| &reader.0)
}
