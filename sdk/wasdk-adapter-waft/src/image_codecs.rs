//! Waft's image codecs over `wasdk:media/image-codecs`, where the system has them, one to one: the
//! formats are the WIT's; an image opened from bytes or from a file, its path inside the program,
//! is the WIT's, what it says of itself read from it as Waft asks; a picture decoded into the
//! program's memory is the WIT's pixel buffer as valo's, field for field; a picture the hardware
//! codecs decoded is the WIT's hardware buffer as Waft's (`hardware_buffers`); and pixels and a
//! hardware buffer encode through the WIT's calls of the same names. What the WIT traps a program
//! for, a picture the image does not have, a size to fit of no pixels and a quality outside 0 to 1,
//! fails here first, as `ImageErrorKind::Other`, as Waft's hosts answer it; valo's pixel buffers
//! always hold their rows.
//!
//! The invariant: each call under way holds the WIT's resource it is made on, so an app that lets
//! the codecs or an image go while it is under way still hears it.

use std::any::Any;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use waft_embedder::image_codecs::{
    AlphaType, AnimationFrame, AnimationPlays, DateTime, HardwareImageCodecs, ImageAnimation,
    ImageCodecs, ImageError, ImageErrorKind, ImageFuture, ImageMetadata, ImageOrientation,
    ImagePage, ImagePages, ImagePictures, ImageRotation, ImageSource, Location, PixelBuffer,
    PixelFormat, PixelLayout,
};
use waft_embedder::{HardwareBuffer, Size};
use wasdk::hardware_buffers as buffers;
use wasdk::image_codecs as platform;
use wasdk::windows;

use crate::hardware_buffers::{framework_buffer, wit_buffer};

/// The system's image codecs, with its hardware codecs where it has them.
pub(crate) struct WasdkImageCodecs {
    codecs: Rc<platform::ImageCodecs>,
    hardware: Option<WasdkHardwareImageCodecs>,
}

impl WasdkImageCodecs {
    /// The system's image codecs, where it has them.
    pub(crate) fn get() -> Option<WasdkImageCodecs> {
        let codecs = platform::get_image_codecs()?;
        let hardware = codecs
            .hardware_image_codecs()
            .map(|hardware| WasdkHardwareImageCodecs(Rc::new(hardware)));
        Some(WasdkImageCodecs {
            codecs: Rc::new(codecs),
            hardware,
        })
    }

    /// The WIT's codecs, which Waft's own images decode with too (`image_decoding`).
    pub(crate) fn wit_codecs(&self) -> Rc<platform::ImageCodecs> {
        Rc::clone(&self.codecs)
    }

    /// The WIT's hardware codecs, where the system has them.
    pub(crate) fn wit_hardware_codecs(&self) -> Option<Rc<platform::HardwareImageCodecs>> {
        self.hardware
            .as_ref()
            .map(|hardware| Rc::clone(&hardware.0))
    }
}

impl ImageCodecs for WasdkImageCodecs {
    fn decoded_formats(&self) -> Vec<String> {
        self.codecs.decoded_formats()
    }

    fn encoded_formats(&self) -> Vec<String> {
        self.codecs.encoded_formats()
    }

    fn open(&self, data: Arc<[u8]>) -> ImageFuture<Box<dyn ImageSource>> {
        let codecs = Rc::clone(&self.codecs);
        Box::pin(async move { framework_image(codecs.open(data.to_vec()).await) })
    }

    /// The file at `path`, a path inside the program, as the WIT names it.
    fn open_file(&self, path: &Path) -> ImageFuture<Box<dyn ImageSource>> {
        let codecs = Rc::clone(&self.codecs);
        let path = path.to_string_lossy().into_owned();
        Box::pin(async move { framework_image(codecs.open_file(path).await) })
    }

    fn encode_pixels(
        &self,
        pixels: PixelBuffer,
        format: &str,
        quality: Option<f32>,
    ) -> ImageFuture<Vec<u8>> {
        if let Err(mistake) = check_quality(quality) {
            return failed(mistake);
        }
        let (codecs, format) = (Rc::clone(&self.codecs), format.to_owned());
        Box::pin(async move {
            let pixels = wit_pixels(pixels);
            let encoded = codecs.encode_pixels(pixels, format, quality).await;
            encoded.map_err(framework_image_error)
        })
    }

    fn hardware_image_codecs(&self) -> Option<&dyn HardwareImageCodecs> {
        self.hardware
            .as_ref()
            .map(|hardware| hardware as &dyn HardwareImageCodecs)
    }
}

/// The system's hardware codecs.
struct WasdkHardwareImageCodecs(Rc<platform::HardwareImageCodecs>);

impl HardwareImageCodecs for WasdkHardwareImageCodecs {
    fn decode(
        &self,
        image: &dyn ImageSource,
        index: u32,
        max_size: Option<Size>,
    ) -> ImageFuture<Arc<dyn HardwareBuffer>> {
        let image: &dyn Any = image;
        let Some(image) = image.downcast_ref::<WasdkImage>() else {
            return failed(ImageError::new(
                ImageErrorKind::Unsupported,
                "an image these codecs did not open",
            ));
        };
        if let Err(mistake) = image.check_decoding(index, max_size) {
            return failed(mistake);
        }
        let (codecs, image) = (Rc::clone(&self.0), Rc::clone(&image.0));
        Box::pin(async move {
            let decoded = codecs.decode(&image, index, max_size.map(wit_size)).await;
            decoded.map(framework_buffer).map_err(framework_image_error)
        })
    }

    fn encode(
        &self,
        buffer: &dyn HardwareBuffer,
        format: &str,
        quality: Option<f32>,
    ) -> ImageFuture<Vec<u8>> {
        let Some(buffer) = wit_buffer(buffer) else {
            return failed(ImageError::new(
                ImageErrorKind::Unsupported,
                "a hardware buffer the system did not hand out",
            ));
        };
        if let Err(mistake) = check_quality(quality) {
            return failed(mistake);
        }
        let (codecs, format) = (Rc::clone(&self.0), format.to_owned());
        Box::pin(async move {
            let encoded = codecs.encode(&buffer, format, quality).await;
            encoded.map_err(framework_image_error)
        })
    }
}

/// An image the WIT's codecs opened, shared with each decoding of it under way.
struct WasdkImage(Rc<platform::ImageSource>);

impl WasdkImage {
    /// Whether `index` and `max_size` are the app's to ask of the image: why not, where the WIT
    /// would trap.
    fn check_decoding(&self, index: u32, max_size: Option<Size>) -> Result<(), ImageError> {
        let pictures = picture_count(&self.0.pictures());
        if index as usize >= pictures {
            return Err(ImageError::new(
                ImageErrorKind::Other,
                format!("picture {index} of an image of {pictures} pictures"),
            ));
        }
        match max_size {
            Some(size) if !(size.width() >= 1.0 && size.height() >= 1.0) => Err(ImageError::new(
                ImageErrorKind::Other,
                format!("no image fits in {size:?}"),
            )),
            _ => Ok(()),
        }
    }
}

impl ImageSource for WasdkImage {
    fn mime_type(&self) -> String {
        self.0.mime_type()
    }

    fn size(&self) -> Size {
        let size = self.0.size();
        Size::new(size.width, size.height)
    }

    fn pictures(&self) -> ImagePictures {
        framework_pictures(self.0.pictures())
    }

    fn orientation(&self) -> ImageOrientation {
        framework_orientation(self.0.orientation())
    }

    fn metadata(&self) -> ImageMetadata {
        framework_metadata(self.0.metadata())
    }

    fn decode_pixels(&self, index: u32, max_size: Option<Size>) -> ImageFuture<PixelBuffer> {
        if let Err(mistake) = self.check_decoding(index, max_size) {
            return failed(mistake);
        }
        let image = Rc::clone(&self.0);
        Box::pin(async move {
            let decoded = image.decode_pixels(index, max_size.map(wit_size)).await;
            framework_pixels(decoded.map_err(framework_image_error)?)
        })
    }
}

/// Why a quality outside 0 to 1 is the app's mistake, which the WIT would trap.
fn check_quality(quality: Option<f32>) -> Result<(), ImageError> {
    match quality {
        Some(quality) if !(0.0..=1.0).contains(&quality) => Err(ImageError::new(
            ImageErrorKind::Other,
            format!("a quality of {quality}, outside 0 to 1"),
        )),
        _ => Ok(()),
    }
}

fn failed<T: 'static>(error: ImageError) -> ImageFuture<T> {
    Box::pin(std::future::ready(Err(error)))
}

/// The WIT's opened image as Waft's, or why it did not open.
fn framework_image(
    opened: Result<platform::ImageSource, buffers::ImageError>,
) -> Result<Box<dyn ImageSource>, ImageError> {
    let image = opened.map_err(framework_image_error)?;
    Ok(Box::new(WasdkImage(Rc::new(image))))
}

fn wit_size(size: Size) -> windows::Size {
    windows::Size {
        width: size.width(),
        height: size.height(),
    }
}

/// valo's pixels as the WIT's pixel buffer, field for field.
fn wit_pixels(pixels: PixelBuffer) -> buffers::PixelBuffer {
    let layout = pixels.layout();
    buffers::PixelBuffer {
        width: layout.size[0],
        height: layout.size[1],
        row_bytes: layout.row_bytes,
        format: wit_format(layout.format),
        alpha: match layout.alpha {
            AlphaType::Premultiplied => buffers::AlphaType::Premultiplied,
            AlphaType::Straight => buffers::AlphaType::Straight,
        },
        bytes: pixels.into_bytes(),
    }
}

/// valo's pixel format as the WIT's.
fn wit_format(format: PixelFormat) -> buffers::PixelFormat {
    match format {
        PixelFormat::Rgba8 => buffers::PixelFormat::Rgba8,
        PixelFormat::Bgra8 => buffers::PixelFormat::Bgra8,
    }
}

/// The WIT's pixel buffer as valo's, field for field; a failure for one whose bytes do not hold
/// its rows, which the WIT never answers.
pub(crate) fn framework_pixels(pixels: buffers::PixelBuffer) -> Result<PixelBuffer, ImageError> {
    let layout = PixelLayout {
        size: [pixels.width, pixels.height],
        row_bytes: pixels.row_bytes,
        format: match pixels.format {
            buffers::PixelFormat::Rgba8 => PixelFormat::Rgba8,
            buffers::PixelFormat::Bgra8 => PixelFormat::Bgra8,
        },
        alpha: match pixels.alpha {
            buffers::AlphaType::Premultiplied => AlphaType::Premultiplied,
            buffers::AlphaType::Straight => AlphaType::Straight,
        },
    };
    PixelBuffer::new(layout, pixels.bytes)
        .map_err(|error| ImageError::new(ImageErrorKind::Other, format!("{error:?}")))
}

pub(crate) fn framework_image_error(error: buffers::ImageError) -> ImageError {
    let kind = match error.kind {
        buffers::ImageErrorKind::Unsupported => ImageErrorKind::Unsupported,
        buffers::ImageErrorKind::Damaged => ImageErrorKind::Damaged,
        buffers::ImageErrorKind::TooLarge => ImageErrorKind::TooLarge,
        buffers::ImageErrorKind::NotFound => ImageErrorKind::NotFound,
        buffers::ImageErrorKind::OutOfMemory => ImageErrorKind::OutOfMemory,
        buffers::ImageErrorKind::Other => ImageErrorKind::Other,
    };
    ImageError::new(kind, error.message)
}

/// How the WIT says an image's pictures go, in Waft's terms.
fn framework_pictures(pictures: platform::ImagePictures) -> ImagePictures {
    match pictures {
        platform::ImagePictures::Still => ImagePictures::Still,
        platform::ImagePictures::Animation(animation) => ImagePictures::Animation(ImageAnimation {
            frames: animation.frames.into_iter().map(framework_frame).collect(),
            plays: match animation.plays {
                platform::AnimationPlays::Times(times) => AnimationPlays::Times(times),
                platform::AnimationPlays::Forever => AnimationPlays::Forever,
            },
        }),
        platform::ImagePictures::Pages(pages) => ImagePictures::Pages(ImagePages {
            pages: pages.pages.into_iter().map(framework_page).collect(),
            primary: pages.primary,
        }),
    }
}

fn framework_frame(frame: platform::AnimationFrame) -> AnimationFrame {
    AnimationFrame {
        duration: frame.duration.map(Duration::from_nanos),
    }
}

fn framework_page(page: platform::ImagePage) -> ImagePage {
    ImagePage {
        size: Size::new(page.size.width, page.size.height),
        orientation: framework_orientation(page.orientation),
    }
}

/// How many pictures the WIT's `pictures` holds: one still, an animation's frames or a document's
/// pages.
fn picture_count(pictures: &platform::ImagePictures) -> usize {
    match pictures {
        platform::ImagePictures::Still => 1,
        platform::ImagePictures::Animation(animation) => animation.frames.len(),
        platform::ImagePictures::Pages(pages) => pages.pages.len(),
    }
}

fn framework_orientation(orientation: platform::ImageOrientation) -> ImageOrientation {
    ImageOrientation {
        mirrored: orientation.mirrored,
        rotation: match orientation.rotation {
            platform::ImageRotation::None => ImageRotation::None,
            platform::ImageRotation::QuarterTurn => ImageRotation::QuarterTurn,
            platform::ImageRotation::HalfTurn => ImageRotation::HalfTurn,
            platform::ImageRotation::ThreeQuarterTurn => ImageRotation::ThreeQuarterTurn,
        },
    }
}

fn framework_metadata(metadata: platform::ImageMetadata) -> ImageMetadata {
    ImageMetadata {
        taken: metadata.taken.map(|taken| DateTime {
            year: taken.year,
            month: taken.month,
            day: taken.day,
            hour: taken.hour,
            minute: taken.minute,
            second: taken.second,
            utc_offset: taken.utc_offset,
        }),
        camera_make: metadata.camera_make,
        camera_model: metadata.camera_model,
        lens_model: metadata.lens_model,
        exposure_time: metadata.exposure_time,
        f_number: metadata.f_number,
        iso: metadata.iso,
        focal_length: metadata.focal_length,
        equivalent_focal_length: metadata.equivalent_focal_length,
        exposure_bias: metadata.exposure_bias,
        flash_fired: metadata.flash_fired,
        location: metadata.location.map(|location| Location {
            latitude: location.latitude,
            longitude: location.longitude,
            altitude: location.altitude,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_go_over_field_for_field_both_ways() {
        let layout = PixelLayout {
            size: [2, 3],
            row_bytes: 12,
            format: PixelFormat::Bgra8,
            alpha: AlphaType::Straight,
        };
        let bytes: Vec<u8> = (0..36).collect();
        let pixels = PixelBuffer::new(layout, bytes.clone()).unwrap();
        let wit = wit_pixels(pixels);
        assert_eq!((wit.width, wit.height, wit.row_bytes), (2, 3, 12));
        assert!(matches!(wit.format, buffers::PixelFormat::Bgra8));
        assert!(matches!(wit.alpha, buffers::AlphaType::Straight));
        let back = framework_pixels(wit).unwrap();
        assert_eq!(back.layout(), layout);
        assert_eq!(back.bytes(), bytes);
    }

    #[test]
    fn a_quality_outside_0_to_1_is_the_apps_mistake() {
        assert!(check_quality(Some(0.5)).is_ok());
        assert!(check_quality(None).is_ok());
        let mistake = check_quality(Some(-0.1)).unwrap_err();
        assert_eq!(mistake.kind, ImageErrorKind::Other);
    }
}
