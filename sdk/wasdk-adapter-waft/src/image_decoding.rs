//! Waft's own images decoded: `Platform::open_image_codec`, dart:ui's `instantiateImageCodec`, which
//! a `MemoryImage`, an `AssetImage` and every other image provider of Waft's decodes through, over
//! `wasdk:media/image-codecs`. The bytes open as the WIT's image, and each frame decodes as the
//! framework asks for it: by the hardware codecs, where the system has them, into a hardware buffer
//! imported as a texture with no copy through the program's memory, as Waft's native hosts take a
//! frame the system's codec decoded on the GPU, and into the program's memory elsewhere, uploaded.
//! Either becomes an image on the renderer's device with its mip levels, as Waft's native hosts
//! decode it, since a picture is routinely drawn smaller than it decodes.
//!
//! The frames are the WIT's pictures: a still image's one and a document's primary page are one
//! frame, shown once, as Waft shows one picture of a document, and an animation's frames are shown
//! each for as long as it says, and for no time where it says nothing, for which Waft picks its
//! own, played as often as the image says.
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
    decoder: FrameDecoder,
}

impl ImageDecoding {
    /// Decoding with `codecs`, the system's, into images on `renderer`'s device.
    pub(crate) fn new(codecs: &WasdkImageCodecs, renderer: Rc<Renderer>) -> ImageDecoding {
        ImageDecoding {
            codecs: codecs.wit_codecs(),
            decoder: FrameDecoder {
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
        let (codecs, decoder) = (Rc::clone(&self.codecs), self.decoder.clone());
        Box::pin(async move {
            let image = codecs.open(bytes.to_vec()).await.map_err(decode_error)?;
            let codec = WasdkImageCodec::new(image, decoder)?;
            Ok(Box::new(codec) as Box<dyn ImageCodec>)
        })
    }
}

/// One image the system's codecs opened, giving up its frames in order, the first again after the
/// last.
struct WasdkImageCodec {
    image: platform::ImageSource,
    /// Waft's way through its pictures: the frames it gives, in order, and what happens after the
    /// last.
    playing: Playing,
    decoder: FrameDecoder,
    /// The frame the next `next_frame` decodes.
    next: u32,
}

/// Waft's way through an image's pictures: the frames it gives, in order, and what happens after
/// the last.
struct Playing {
    frames: Vec<Frame>,
    repetition: ImageRepetition,
}

/// One frame the codec gives: which of the image's pictures it decodes, and how long it shows.
#[derive(Clone, Copy)]
struct Frame {
    picture: u32,
    duration: Duration,
}

impl WasdkImageCodec {
    /// The codec of `image`, decoding its pictures with `decoder`; a failure for an image of no
    /// frames, which the WIT never opens.
    fn new(
        image: platform::ImageSource,
        decoder: FrameDecoder,
    ) -> Result<WasdkImageCodec, ImageDecodeError> {
        let playing = playing(image.pictures());
        if playing.frames.is_empty() {
            return Err(ImageDecodeError::Empty);
        }
        Ok(WasdkImageCodec {
            image,
            playing,
            decoder,
            next: 0,
        })
    }

    /// The frame to decode now, the one after it becoming the next.
    fn take_next(&mut self) -> Frame {
        let frame = self.playing.frames[self.next as usize];
        self.next = (self.next + 1) % self.frame_count();
        frame
    }
}

/// Waft's way through `pictures`: a still image's one picture and a document's primary page are one
/// frame, shown once, as Waft shows one picture of a document; an animation's frames are shown as
/// often as it says, Waft counting the repeats after the first play.
fn playing(pictures: platform::ImagePictures) -> Playing {
    let one = |picture| Playing {
        frames: vec![Frame {
            picture,
            duration: Duration::ZERO,
        }],
        repetition: ImageRepetition::Once,
    };
    match pictures {
        platform::ImagePictures::Still => one(0),
        platform::ImagePictures::Animation(animation) => Playing {
            frames: animation
                .frames
                .into_iter()
                .enumerate()
                .map(|(index, frame)| Frame {
                    picture: index as u32,
                    duration: frame.duration.map_or(Duration::ZERO, Duration::from_nanos),
                })
                .collect(),
            repetition: match animation.plays {
                platform::AnimationPlays::Times(times) if times <= 1 => ImageRepetition::Once,
                platform::AnimationPlays::Times(times) => ImageRepetition::Times(times - 1),
                platform::AnimationPlays::Forever => ImageRepetition::Forever,
            },
        },
        platform::ImagePictures::Pages(pages) => one(pages.primary),
    }
}

impl ImageCodec for WasdkImageCodec {
    fn frame_count(&self) -> u32 {
        self.playing.frames.len() as u32
    }

    fn repetition(&self) -> ImageRepetition {
        self.playing.repetition
    }

    fn next_frame(&mut self) -> ImageFrameFuture<'_> {
        let frame = self.take_next();
        Box::pin(async move {
            let image = self.decoder.decode(&self.image, frame.picture).await?;
            Ok(ImageFrame {
                image,
                duration: frame.duration,
            })
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
    /// Picture `index` of `image` at its full size, as an image on the renderer's device.
    async fn decode(
        &self,
        image: &platform::ImageSource,
        index: u32,
    ) -> Result<valo::Image, ImageDecodeError> {
        match &self.hardware {
            Some(hardware) => self.decode_into_hardware(hardware, image, index).await,
            None => self.decode_into_memory(image, index).await,
        }
    }

    async fn decode_into_hardware(
        &self,
        hardware: &platform::HardwareImageCodecs,
        image: &platform::ImageSource,
        index: u32,
    ) -> Result<valo::Image, ImageDecodeError> {
        let buffer = hardware
            .decode(image, index, None)
            .await
            .map_err(decode_error)?;
        self.renderer
            .import_buffer(&*framework_buffer(buffer), true)
            .ok_or_else(|| not_drawable("the hardware buffer it decoded into"))
    }

    async fn decode_into_memory(
        &self,
        image: &platform::ImageSource,
        index: u32,
    ) -> Result<valo::Image, ImageDecodeError> {
        let pixels = image
            .decode_pixels(index, None)
            .await
            .map_err(decode_error)?;
        let pixels =
            framework_pixels(pixels).map_err(|error| ImageDecodeError::Failed(error.message))?;
        self.renderer
            .import_pixels(pixels, true)
            .ok_or_else(|| not_drawable("the pixels it decoded"))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use platform::{AnimationFrame, AnimationPlays, ImageAnimation, ImagePages};

    /// An animation of two frames, one shown for 100 ms and one whose file says nothing, that plays
    /// as `plays` says.
    fn animation(plays: AnimationPlays) -> platform::ImagePictures {
        platform::ImagePictures::Animation(ImageAnimation {
            frames: vec![
                AnimationFrame {
                    duration: Some(100_000_000),
                },
                AnimationFrame { duration: None },
            ],
            plays,
        })
    }

    /// A page of `side` by `side`, upright.
    fn page(side: f64) -> platform::ImagePage {
        platform::ImagePage {
            size: wasdk::windows::Size {
                width: side,
                height: side,
            },
            orientation: platform::ImageOrientation {
                mirrored: false,
                rotation: platform::ImageRotation::None,
            },
        }
    }

    #[test]
    fn a_still_image_is_one_frame_shown_once() {
        let playing = playing(platform::ImagePictures::Still);
        assert_eq!(playing.frames.len(), 1);
        assert_eq!(playing.frames[0].picture, 0);
        assert_eq!(playing.frames[0].duration, Duration::ZERO);
        assert_eq!(playing.repetition, ImageRepetition::Once);
    }

    #[test]
    fn an_animations_frames_play_as_often_as_it_says() {
        let once = playing(animation(AnimationPlays::Times(1)));
        assert_eq!(once.frames.len(), 2);
        assert_eq!(once.repetition, ImageRepetition::Once);

        let thrice = playing(animation(AnimationPlays::Times(3)));
        assert_eq!(thrice.frames.len(), 2);
        assert_eq!(thrice.frames[0].picture, 0);
        assert_eq!(thrice.frames[1].picture, 1);
        assert_eq!(thrice.frames[0].duration, Duration::from_millis(100));
        assert_eq!(thrice.frames[1].duration, Duration::ZERO);
        assert_eq!(
            thrice.repetition,
            ImageRepetition::Times(2),
            "Waft counts the repeats after the first play"
        );

        let forever = playing(animation(AnimationPlays::Forever));
        assert_eq!(forever.frames.len(), 2);
        assert_eq!(forever.repetition, ImageRepetition::Forever);
    }

    #[test]
    fn a_documents_primary_page_is_one_frame_shown_once() {
        let pages = platform::ImagePictures::Pages(ImagePages {
            pages: vec![page(1.0), page(2.0)],
            primary: 1,
        });
        let playing = playing(pages);
        assert_eq!(playing.frames.len(), 1);
        assert_eq!(playing.frames[0].picture, 1);
        assert_eq!(playing.frames[0].duration, Duration::ZERO);
        assert_eq!(playing.repetition, ImageRepetition::Once);
    }
}
