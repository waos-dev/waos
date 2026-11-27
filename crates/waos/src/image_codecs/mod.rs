//! `wasdk:media/image-codecs`: a program's images decoded and encoded by the system's codecs, into
//! its own memory and out of it, and where the system can, into hardware buffers and out of them.
//!
//! The shell decodes and encodes ([`ShellImageCodecs`], each image a `ShellImageSource`, the
//! hardware codecs a `ShellHardwareImageCodecs`); the engine keeps the rules. An image's file is
//! looked up among the program's mounts, as its files are, and the shell is handed where it is on
//! the machine; what an image says of itself is answered from what the shell said as it opened,
//! kept with it; a hardware buffer the hardware codecs decode into is the program's own, counted
//! in its stats (`hardware_buffers`), held by the program until it lets it go; and a buffer is
//! encoded once the GPU has finished what the program submitted and wrote to draw into it. The
//! interface is served to a program by `host`.
//!
//! The invariant: the program's mistakes reach no shell, and trap: a picture its image does not
//! have, pixels whose bytes do not hold their rows, a quality outside 0 to 1 and a size to fit of
//! no pixels.

pub(crate) mod host;

use std::path::Path;
use std::sync::Arc;

use wasmtime::bail;

use crate::access::Cause;
use crate::bindings::wasdk::media::image_codecs::{ImageMetadata, ImageOrientation, ImagePictures};
use crate::bindings::wasdk::platform::hardware_buffers::{ImageError, ImageErrorKind, PixelBuffer};
use crate::bindings::wasdk::platform::windows::Size;
use crate::files::ProgramFiles;
use crate::running::RunningProgram;
use crate::shell::{
    HardwareBuffer, ImageFuture, OpenedImage, ShellHardwareImageCodecs, ShellImageCodecs,
    ShellImageSource,
};

/// `image-codecs.image-codecs`: the shell's codecs, as the program reaches them.
pub struct ImageCodecs(Arc<dyn ShellImageCodecs>);

/// `image-codecs.image-source`: an image the shell's codecs opened, with what it said of itself
/// as it opened.
pub struct ImageSource {
    shell: Arc<dyn ShellImageSource>,
    mime_type: String,
    size: Size,
    pictures: ImagePictures,
    orientation: ImageOrientation,
    metadata: ImageMetadata,
}

/// `image-codecs.hardware-image-codecs`: the shell's hardware codecs, as the program reaches them.
pub struct HardwareImageCodecs(Arc<dyn ShellHardwareImageCodecs>);

impl ImageCodecs {
    pub(crate) fn of(shell: Arc<dyn ShellImageCodecs>) -> ImageCodecs {
        ImageCodecs(shell)
    }

    fn decoded_formats(&self) -> Vec<String> {
        self.0.decoded_formats()
    }

    fn encoded_formats(&self) -> Vec<String> {
        self.0.encoded_formats()
    }

    /// The image `data` encodes, opened by the shell for `program`.
    fn open(&self, program: &RunningProgram, data: Vec<u8>) -> ImageFuture<ImageSource> {
        opened(self.0.open(program, data))
    }

    /// The image in the program's file at `path`, opened by the shell where its mount puts it on
    /// the machine; `not-found`, the shell asked nothing, for a path with no file among the
    /// program's.
    fn open_file(
        &self,
        program: &RunningProgram,
        files: &mut ProgramFiles,
        path: &str,
    ) -> ImageFuture<ImageSource> {
        match files.machine_path(Path::new(path), Cause::ImageCodecs) {
            Some(machine_path) => opened(self.0.open_file(program, &machine_path)),
            None => Box::pin(std::future::ready(Err(ImageError {
                kind: ImageErrorKind::NotFound,
                message: format!("no file at {path} among the program's"),
            }))),
        }
    }

    /// `pixels` encoded by the shell; a trap for pixels whose bytes do not hold their rows, or a
    /// quality outside 0 to 1.
    fn encode_pixels(
        &self,
        pixels: PixelBuffer,
        format: &str,
        quality: Option<f32>,
    ) -> wasmtime::Result<ImageFuture<Vec<u8>>> {
        check_pixels(&pixels)?;
        check_quality(quality)?;
        Ok(self.0.encode_pixels(pixels, format, quality))
    }

    fn hardware_image_codecs(&self) -> Option<HardwareImageCodecs> {
        self.0.hardware_image_codecs().map(HardwareImageCodecs)
    }
}

/// The image `opening` answers, with what it said of itself.
fn opened(opening: ImageFuture<OpenedImage>) -> ImageFuture<ImageSource> {
    Box::pin(async move { opening.await.map(ImageSource::of) })
}

impl ImageSource {
    fn of(opened: OpenedImage) -> ImageSource {
        ImageSource {
            shell: Arc::from(opened.image),
            mime_type: opened.mime_type,
            size: opened.size,
            pictures: opened.pictures,
            orientation: opened.orientation,
            metadata: opened.metadata,
        }
    }

    fn mime_type(&self) -> String {
        self.mime_type.clone()
    }

    fn size(&self) -> Size {
        self.size
    }

    fn pictures(&self) -> ImagePictures {
        self.pictures.clone()
    }

    fn orientation(&self) -> ImageOrientation {
        self.orientation
    }

    fn metadata(&self) -> ImageMetadata {
        self.metadata.clone()
    }

    /// Picture `index` decoded by the shell into the program's memory; a trap for a picture the
    /// image does not have, or a size to fit of no pixels.
    fn decode_pixels(
        &self,
        index: u32,
        max_size: Option<Size>,
    ) -> wasmtime::Result<ImageFuture<PixelBuffer>> {
        self.check_decoding(index, max_size)?;
        Ok(self.shell.decode_pixels(index, max_size))
    }

    /// Whether `index` and `max_size` are the program's to ask: a trap otherwise.
    fn check_decoding(&self, index: u32, max_size: Option<Size>) -> wasmtime::Result<()> {
        let pictures = picture_count(&self.pictures);
        if index as usize >= pictures {
            bail!("the program decoded picture {index} of an image of {pictures} pictures");
        }
        if let Some(size) = max_size
            && !(size.width >= 1.0 && size.height >= 1.0)
        {
            bail!(
                "the program decoded an image to fit {}x{}, a size of no pixels",
                size.width,
                size.height
            );
        }
        Ok(())
    }
}

impl HardwareImageCodecs {
    /// Picture `index` of `image` decoded by the shell into a hardware buffer of the program's own;
    /// a trap as [`ImageSource::decode_pixels`] traps.
    fn decode(
        &self,
        image: &ImageSource,
        index: u32,
        max_size: Option<Size>,
    ) -> wasmtime::Result<ImageFuture<HardwareBuffer>> {
        image.check_decoding(index, max_size)?;
        Ok(self.0.decode(image.shell.as_ref(), index, max_size))
    }

    /// The shell's codecs, to encode a buffer at `quality` once the GPU work that drew into it is
    /// done; a trap for a quality outside 0 to 1.
    fn encoder(&self, quality: Option<f32>) -> wasmtime::Result<Arc<dyn ShellHardwareImageCodecs>> {
        check_quality(quality)?;
        Ok(Arc::clone(&self.0))
    }
}

/// How many pictures `pictures` holds: one still, an animation's frames or a document's pages.
fn picture_count(pictures: &ImagePictures) -> usize {
    match pictures {
        ImagePictures::Still => 1,
        ImagePictures::Animation(animation) => animation.frames.len(),
        ImagePictures::Pages(pages) => pages.pages.len(),
    }
}

/// Whether `pixels` hold what they say: at least one pixel, rows of at least four bytes a pixel,
/// and exactly `height` of them; a trap otherwise.
pub(crate) fn check_pixels(pixels: &PixelBuffer) -> wasmtime::Result<()> {
    let row = u64::from(pixels.width) * 4;
    let held = u64::from(pixels.height) * u64::from(pixels.row_bytes);
    if pixels.width == 0 || pixels.height == 0 {
        bail!("the program encoded pixels of no pixels");
    }
    if u64::from(pixels.row_bytes) < row {
        bail!(
            "the program encoded rows of {} bytes, under four for each of {} pixels",
            pixels.row_bytes,
            pixels.width
        );
    }
    if pixels.bytes.len() as u64 != held {
        bail!(
            "the program encoded {} bytes, not {} rows of {}",
            pixels.bytes.len(),
            pixels.height,
            pixels.row_bytes
        );
    }
    Ok(())
}

/// Whether `quality` is from 0 to 1, where given; a trap otherwise.
fn check_quality(quality: Option<f32>) -> wasmtime::Result<()> {
    match quality {
        Some(quality) if !(0.0..=1.0).contains(&quality) => {
            bail!("the program encoded at a quality of {quality}, outside 0 to 1")
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(width: u32, height: u32, row_bytes: u32, length: usize) -> PixelBuffer {
        use crate::bindings::wasdk::platform::hardware_buffers::{AlphaType, PixelFormat};
        PixelBuffer {
            width,
            height,
            row_bytes,
            format: PixelFormat::Rgba8,
            alpha: AlphaType::Premultiplied,
            bytes: vec![0; length],
        }
    }

    #[test]
    fn pixels_must_hold_their_rows_exactly() {
        assert!(check_pixels(&pixels(2, 3, 8, 24)).is_ok());
        assert!(check_pixels(&pixels(2, 3, 16, 48)).is_ok(), "padded rows");
        assert!(
            check_pixels(&pixels(2, 3, 7, 21)).is_err(),
            "a row too short"
        );
        assert!(
            check_pixels(&pixels(2, 3, 8, 23)).is_err(),
            "a byte missing"
        );
        assert!(check_pixels(&pixels(0, 3, 8, 24)).is_err(), "no pixels");
    }

    /// An image whose pictures go as `pictures` say, whose shell is never asked anything.
    fn image_of(pictures: ImagePictures) -> ImageSource {
        struct Unasked;
        impl ShellImageSource for Unasked {
            fn decode_pixels(
                &self,
                _index: u32,
                _max_size: Option<Size>,
            ) -> ImageFuture<PixelBuffer> {
                unreachable!("a mistake reaches no shell")
            }
        }
        use crate::bindings::wasdk::media::image_codecs::ImageRotation;
        ImageSource {
            shell: Arc::new(Unasked),
            mime_type: "image/gif".to_owned(),
            size: Size {
                width: 8.0,
                height: 8.0,
            },
            pictures,
            orientation: ImageOrientation {
                mirrored: false,
                rotation: ImageRotation::None,
            },
            metadata: ImageMetadata {
                taken: None,
                camera_make: None,
                camera_model: None,
                lens_model: None,
                exposure_time: None,
                f_number: None,
                iso: None,
                focal_length: None,
                equivalent_focal_length: None,
                exposure_bias: None,
                flash_fired: None,
                location: None,
            },
        }
    }

    /// An animation of `frames` frames that plays forever.
    fn animation_of(frames: usize) -> ImagePictures {
        use crate::bindings::wasdk::media::image_codecs::{
            AnimationFrame, AnimationPlays, ImageAnimation,
        };
        ImagePictures::Animation(ImageAnimation {
            frames: vec![
                AnimationFrame {
                    duration: Some(100_000_000)
                };
                frames
            ],
            plays: AnimationPlays::Forever,
        })
    }

    #[test]
    fn a_picture_the_image_has_not_and_a_size_of_no_pixels_are_mistakes() {
        let image = image_of(animation_of(3));
        assert!(image.check_decoding(2, None).is_ok());
        let fit = |width, height| Some(Size { width, height });
        assert!(image.check_decoding(0, fit(1.0, 1.0)).is_ok());
        assert!(image.check_decoding(3, None).is_err(), "past its pictures");
        assert!(image.check_decoding(0, fit(0.0, 10.0)).is_err());
        assert!(image.check_decoding(0, fit(10.0, f64::NAN)).is_err());
        assert!(image.decode_pixels(3, None).is_err(), "no shell is asked");
    }

    #[test]
    fn pictures_are_one_still_an_animations_frames_or_a_documents_pages() {
        use crate::bindings::wasdk::media::image_codecs::{ImagePage, ImagePages, ImageRotation};
        assert_eq!(picture_count(&ImagePictures::Still), 1);
        assert_eq!(picture_count(&animation_of(2)), 2);
        let page = ImagePage {
            size: Size {
                width: 8.0,
                height: 8.0,
            },
            orientation: ImageOrientation {
                mirrored: false,
                rotation: ImageRotation::None,
            },
        };
        let pages = ImagePictures::Pages(ImagePages {
            pages: vec![page; 3],
            primary: 1,
        });
        assert_eq!(picture_count(&pages), 3);
        assert!(image_of(pages).check_decoding(3, None).is_err());
    }

    #[test]
    fn a_quality_is_from_0_to_1() {
        assert!(check_quality(None).is_ok());
        assert!(check_quality(Some(0.0)).is_ok());
        assert!(check_quality(Some(1.0)).is_ok());
        assert!(check_quality(Some(1.5)).is_err());
        assert!(check_quality(Some(f32::NAN)).is_err());
    }
}
