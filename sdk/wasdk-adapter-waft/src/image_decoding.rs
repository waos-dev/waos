//! Waft's own images decoded: `Platform::open_image_codec`, dart:ui's `instantiateImageCodec`, which
//! a `MemoryImage`, an `AssetImage` and every other image provider of Waft's decodes through, over
//! `wasdk:media/image-codecs`. The bytes open as the WIT's image, and each frame decodes as the
//! framework asks for it: by the hardware codecs, where the system has them, into a hardware buffer
//! imported as a texture with no copy through the program's memory, as Waft's native hosts take a
//! frame the system's codec decoded on the GPU, and into the program's memory elsewhere, uploaded.
//! Either becomes an image on the renderer's device with its mip levels, as Waft's native hosts
//! decode it, since a picture is routinely drawn smaller than it decodes.
//!
//! The frames are the WIT's, each shown for as long as it says, and for no time where it says
//! nothing, for which Waft picks its own; an image of several frames plays forever, since the WIT
//! carries no count of repeats.
//!
//! The invariant: a codec holds the WIT's image it decodes from, so a frame asked of it decodes
//! while the app holds the codec, and the image goes as the codec does.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use waft_embedder::{
    ImageCodec, ImageCodecFuture, ImageDecodeError, ImageFrame, ImageFrameFuture, ImageRepetition,
};
use wasdk::hardware_buffers as buffers;
use wasdk::image_codecs as platform;

use crate::hardware_buffers::framework_buffer;
use crate::image_codecs::{WasdkImageCodecs, framework_pixels};
use crate::renderer::Renderer;

/// What Waft's own images decode with: the system's image codecs, and the renderer whose device
/// the decoded frames are images on.
pub(crate) struct ImageDecoding {
    codecs: Rc<platform::ImageCodecs>,
    frames: FrameDecoder,
}

impl ImageDecoding {
    /// Decoding with `codecs`, the system's, into images on `renderer`'s device.
    pub(crate) fn new(codecs: &WasdkImageCodecs, renderer: Rc<Renderer>) -> ImageDecoding {
        ImageDecoding {
            codecs: codecs.wit_codecs(),
            frames: FrameDecoder {
                hardware: codecs.wit_hardware_codecs(),
                renderer,
            },
        }
    }

    /// A codec for the image `bytes` encode, once the system's codecs have opened it.
    pub(crate) fn open_codec(&self, bytes: Arc<[u8]>) -> ImageCodecFuture {
        if bytes.is_empty() {
            return Box::pin(std::future::ready(Err(ImageDecodeError::Empty)));
        }
        let (codecs, frames) = (Rc::clone(&self.codecs), self.frames.clone());
        Box::pin(async move {
            let image = codecs.open(bytes.to_vec()).await.map_err(decode_error)?;
            let codec = WasdkImageCodec::new(image, frames)?;
            Ok(Box::new(codec) as Box<dyn ImageCodec>)
        })
    }
}

/// One image the system's codecs opened, giving up its frames in order, the first again after the
/// last.
struct WasdkImageCodec {
    image: platform::ImageSource,
    /// How long each of its frames shows, read once as it opened.
    durations: Vec<Duration>,
    /// The frame the next `next_frame` decodes.
    next: u32,
    frames: FrameDecoder,
}

impl WasdkImageCodec {
    /// The codec of `image`, decoding its frames with `frames`; a failure for an image of no
    /// frames, which the WIT never opens.
    fn new(
        image: platform::ImageSource,
        frames: FrameDecoder,
    ) -> Result<WasdkImageCodec, ImageDecodeError> {
        let durations: Vec<Duration> = image.frames().into_iter().map(frame_duration).collect();
        if durations.is_empty() {
            return Err(ImageDecodeError::Empty);
        }
        Ok(WasdkImageCodec {
            image,
            durations,
            next: 0,
            frames,
        })
    }

    /// The frame to decode now, the one after it becoming the next.
    fn take_next(&mut self) -> u32 {
        let frame = self.next;
        self.next = (frame + 1) % self.frame_count();
        frame
    }
}

impl ImageCodec for WasdkImageCodec {
    fn frame_count(&self) -> u32 {
        self.durations.len() as u32
    }

    fn repetition(&self) -> ImageRepetition {
        if self.frame_count() > 1 {
            ImageRepetition::Forever
        } else {
            ImageRepetition::Once
        }
    }

    fn next_frame(&mut self) -> ImageFrameFuture<'_> {
        let frame = self.take_next();
        let duration = self.durations[frame as usize];
        Box::pin(async move {
            let image = self.frames.decode(&self.image, frame).await?;
            Ok(ImageFrame { image, duration })
        })
    }
}

/// Where a frame decodes: the hardware codecs, where the system has them, or the program's
/// memory, and the renderer the decoded frame becomes an image of.
#[derive(Clone)]
struct FrameDecoder {
    hardware: Option<Rc<platform::HardwareImageCodecs>>,
    renderer: Rc<Renderer>,
}

impl FrameDecoder {
    /// Frame `frame` of `image` at its full size, as an image on the renderer's device.
    async fn decode(
        &self,
        image: &platform::ImageSource,
        frame: u32,
    ) -> Result<valo::Image, ImageDecodeError> {
        match &self.hardware {
            Some(hardware) => self.decode_into_hardware(hardware, image, frame).await,
            None => self.decode_into_memory(image, frame).await,
        }
    }

    async fn decode_into_hardware(
        &self,
        hardware: &platform::HardwareImageCodecs,
        image: &platform::ImageSource,
        frame: u32,
    ) -> Result<valo::Image, ImageDecodeError> {
        let buffer = hardware
            .decode(image, frame, None)
            .await
            .map_err(decode_error)?;
        self.renderer
            .import_buffer(&*framework_buffer(buffer), true)
            .ok_or_else(|| not_drawable("the hardware buffer it decoded into"))
    }

    async fn decode_into_memory(
        &self,
        image: &platform::ImageSource,
        frame: u32,
    ) -> Result<valo::Image, ImageDecodeError> {
        let pixels = image
            .decode_pixels(frame, None)
            .await
            .map_err(decode_error)?;
        let pixels =
            framework_pixels(pixels).map_err(|error| ImageDecodeError::Failed(error.message))?;
        self.renderer
            .import_pixels(pixels, true)
            .ok_or_else(|| not_drawable("the pixels it decoded"))
    }
}

/// How long a frame shows: none, for a still image or a file that says nothing, as zero.
fn frame_duration(frame: platform::ImageFrame) -> Duration {
    frame.duration.map_or(Duration::ZERO, Duration::from_nanos)
}

/// Why the renderer took no image of `what`.
fn not_drawable(what: &str) -> ImageDecodeError {
    ImageDecodeError::Failed(format!("the renderer made no image of {what}"))
}

/// The WIT's image error in the words Waft's native hosts give a decoder's: a format the system
/// does not decode is an unknown one, bytes it cannot read are damaged, and anything else fails.
fn decode_error(error: buffers::ImageError) -> ImageDecodeError {
    match error.kind {
        buffers::ImageErrorKind::Unsupported => ImageDecodeError::UnknownFormat,
        buffers::ImageErrorKind::Damaged => ImageDecodeError::Damaged(error.message),
        _ => ImageDecodeError::Failed(error.message),
    }
}
