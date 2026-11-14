//! The system's image codecs as the test shell keeps them: each image opened recorded with the
//! program that opened it and what it was opened from, a file's path on the machine or its bytes,
//! and each decoding and encoding asked of them, in order. Every image says what the test set, and
//! decodes into pixels of one colour, or, by the hardware codecs, into an `IOSurface` of that
//! colour, a hardware buffer of the program's own; every encoding answers the bytes the
//! test set, and an encoded hardware buffer is kept, so a test reads what the program handed over.

use std::sync::{Arc, Mutex};

use waos::bindings::wasdk::media::image_codecs::{
    ImageFrame, ImageMetadata, ImageOrientation, ImageRotation,
};
use waos::bindings::wasdk::platform::hardware_buffers::{
    AlphaType, ImageError, ImageErrorKind, PixelBuffer, PixelFormat,
};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{
    HardwareBuffer, ImageFuture, OpenedImage, RunningProgram, ShellHardwareImageCodecs,
    ShellImageCodecs, ShellImageSource,
};

/// Image codecs that answer what the test set, with hardware codecs or without.
pub struct TestImageCodecs {
    shared: Arc<Shared>,
    hardware: Option<Arc<TestHardwareImageCodecs>>,
}

struct Shared {
    answers: Mutex<ImageAnswers>,
    asked: Mutex<Vec<Asked>>,
}

/// What every image and every encoding answers. Each fails until the test sets it.
#[derive(Clone)]
pub struct ImageAnswers {
    pub decoded_formats: Vec<String>,
    pub encoded_formats: Vec<String>,
    /// What an opened image says of itself, or why it did not open.
    pub opened: Result<ImageFacts, ImageError>,
    /// The colour of every decoded pixel, as the bytes of an RGBA pixel.
    pub pixels: Result<[u8; 4], ImageError>,
    pub encoded: Result<Vec<u8>, ImageError>,
}

/// What an image says of itself.
#[derive(Clone)]
pub struct ImageFacts {
    pub mime_type: String,
    pub size: (f64, f64),
    pub frames: Vec<ImageFrame>,
    pub orientation: ImageOrientation,
    pub metadata: ImageMetadata,
}

impl ImageFacts {
    /// A still image of `width` by `height` in `mime_type`, upright, its file saying nothing of
    /// how it was taken.
    pub fn still(mime_type: &str, width: f64, height: f64) -> ImageFacts {
        ImageFacts {
            mime_type: mime_type.to_owned(),
            size: (width, height),
            frames: vec![ImageFrame { duration: None }],
            orientation: ImageOrientation {
                mirrored: false,
                rotation: ImageRotation::None,
            },
            metadata: no_metadata(),
        }
    }
}

/// A file that says nothing of how it was taken.
pub fn no_metadata() -> ImageMetadata {
    ImageMetadata {
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
    }
}

impl Default for ImageAnswers {
    fn default() -> ImageAnswers {
        ImageAnswers {
            decoded_formats: Vec::new(),
            encoded_formats: Vec::new(),
            opened: Err(unset()),
            pixels: Err(unset()),
            encoded: Err(unset()),
        }
    }
}

fn unset() -> ImageError {
    ImageError {
        kind: ImageErrorKind::Other,
        message: "the test set no answer".to_owned(),
    }
}

/// What a program asked of the codecs, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Asked {
    /// An image opened by the program named, from what it says.
    Open { program: String, from: OpenedFrom },
    Decode {
        frame: u32,
        max_size: Option<(f64, f64)>,
        hardware: bool,
    },
    /// Pixels encoded, or a hardware buffer, its first pixel's bytes as they lie, BGRA.
    Encode {
        width: u32,
        height: u32,
        format: String,
        quality: Option<f32>,
        hardware_first_pixel: Option<[u8; 4]>,
    },
}

/// What an image was opened from.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenedFrom {
    /// Bytes, of this length.
    Bytes(usize),
    /// A file, at this path on the machine.
    File(std::path::PathBuf),
}

impl TestImageCodecs {
    /// Codecs with hardware codecs, as on Apple's systems.
    pub fn with_hardware() -> TestImageCodecs {
        let shared = Arc::new(Shared {
            answers: Mutex::default(),
            asked: Mutex::default(),
        });
        let hardware = Arc::new(TestHardwareImageCodecs(Arc::clone(&shared)));
        TestImageCodecs {
            shared,
            hardware: Some(hardware),
        }
    }

    /// Codecs without hardware codecs, as a system that decodes only into a program's memory.
    pub fn without_hardware() -> TestImageCodecs {
        TestImageCodecs {
            hardware: None,
            ..TestImageCodecs::with_hardware()
        }
    }

    /// Every call answers `answers` from now on.
    pub fn answer_with(&self, answers: ImageAnswers) {
        *self.shared.answers.lock().unwrap() = answers;
    }

    /// What was asked so far, in order.
    pub fn asked(&self) -> Vec<Asked> {
        self.shared.asked.lock().unwrap().clone()
    }
}

impl Default for TestImageCodecs {
    fn default() -> TestImageCodecs {
        TestImageCodecs::with_hardware()
    }
}

impl Shared {
    fn answers(&self) -> ImageAnswers {
        self.answers.lock().unwrap().clone()
    }

    fn ask(&self, asked: Asked) {
        self.asked.lock().unwrap().push(asked);
    }

    fn opened(
        self: &Arc<Self>,
        program: &RunningProgram,
        from: OpenedFrom,
    ) -> ImageFuture<OpenedImage> {
        self.ask(Asked::Open {
            program: program.name().to_owned(),
            from,
        });
        let opened = self.answers().opened.map(|facts| OpenedImage {
            image: Box::new(TestImage {
                shared: Arc::clone(self),
                size: facts.size,
            }),
            mime_type: facts.mime_type,
            size: Size {
                width: facts.size.0,
                height: facts.size.1,
            },
            frames: facts.frames,
            orientation: facts.orientation,
            metadata: facts.metadata,
        });
        Box::pin(std::future::ready(opened))
    }

    fn encoded(
        &self,
        [width, height]: [u32; 2],
        format: &str,
        quality: Option<f32>,
        hardware_first_pixel: Option<[u8; 4]>,
    ) -> ImageFuture<Vec<u8>> {
        self.ask(Asked::Encode {
            width,
            height,
            format: format.to_owned(),
            quality,
            hardware_first_pixel,
        });
        Box::pin(std::future::ready(self.answers().encoded))
    }
}

impl ShellImageCodecs for TestImageCodecs {
    fn decoded_formats(&self) -> Vec<String> {
        self.shared.answers().decoded_formats
    }

    fn encoded_formats(&self) -> Vec<String> {
        self.shared.answers().encoded_formats
    }

    fn open(&self, program: &RunningProgram, data: Vec<u8>) -> ImageFuture<OpenedImage> {
        self.shared.opened(program, OpenedFrom::Bytes(data.len()))
    }

    fn open_file(
        &self,
        program: &RunningProgram,
        path: &std::path::Path,
    ) -> ImageFuture<OpenedImage> {
        self.shared
            .opened(program, OpenedFrom::File(path.to_owned()))
    }

    fn encode_pixels(
        &self,
        pixels: PixelBuffer,
        format: &str,
        quality: Option<f32>,
    ) -> ImageFuture<Vec<u8>> {
        self.shared
            .encoded([pixels.width, pixels.height], format, quality, None)
    }

    fn hardware_image_codecs(&self) -> Option<Arc<dyn ShellHardwareImageCodecs>> {
        self.hardware
            .clone()
            .map(|hardware| hardware as Arc<dyn ShellHardwareImageCodecs>)
    }
}

/// An image a program opened.
struct TestImage {
    shared: Arc<Shared>,
    size: (f64, f64),
}

impl ShellImageSource for TestImage {
    fn decode_pixels(&self, frame: u32, max_size: Option<Size>) -> ImageFuture<PixelBuffer> {
        self.shared.ask(Asked::Decode {
            frame,
            max_size: max_size.map(|size| (size.width, size.height)),
            hardware: false,
        });
        let [width, height] = fitted(self.size, max_size);
        let pixels = self.shared.answers().pixels.map(|rgba| PixelBuffer {
            width,
            height,
            row_bytes: width * 4,
            format: PixelFormat::Rgba8,
            alpha: AlphaType::Premultiplied,
            bytes: rgba.repeat(width as usize * height as usize),
        });
        Box::pin(std::future::ready(pixels))
    }
}

/// The hardware codecs of [`TestImageCodecs::with_hardware`].
struct TestHardwareImageCodecs(Arc<Shared>);

impl ShellHardwareImageCodecs for TestHardwareImageCodecs {
    fn decode(
        &self,
        image: &dyn ShellImageSource,
        frame: u32,
        max_size: Option<Size>,
    ) -> ImageFuture<HardwareBuffer> {
        self.0.ask(Asked::Decode {
            frame,
            max_size: max_size.map(|size| (size.width, size.height)),
            hardware: true,
        });
        let image: &dyn std::any::Any = image;
        let size = image
            .downcast_ref::<TestImage>()
            .expect("an image the test codecs opened")
            .size;
        decoded(fitted(size, max_size), self.0.answers().pixels)
    }

    fn encode(
        &self,
        buffer: &HardwareBuffer,
        format: &str,
        quality: Option<f32>,
    ) -> ImageFuture<Vec<u8>> {
        let (size, first_pixel) = read(buffer);
        self.0.encoded(size, format, quality, Some(first_pixel))
    }
}

/// `size` scaled down to fit `max_size`, keeping its shape, in whole pixels, at least one each way.
fn fitted((width, height): (f64, f64), max_size: Option<Size>) -> [u32; 2] {
    let scale = max_size.map_or(1.0, |max| {
        (max.width / width).min(max.height / height).min(1.0)
    });
    let side = |length: f64| ((length * scale).round() as u32).max(1);
    [side(width), side(height)]
}

/// A hardware buffer of `size` filled with `pixels`' colour, or its failure.
#[cfg(target_vendor = "apple")]
fn decoded(
    [width, height]: [u32; 2],
    pixels: Result<[u8; 4], ImageError>,
) -> ImageFuture<HardwareBuffer> {
    use super::hardware_buffer::{HeldHardwareBuffer, surface_filled};

    match pixels {
        Ok([red, green, blue, alpha]) => {
            let held = HeldHardwareBuffer(surface_filled(
                width as usize,
                height as usize,
                [blue, green, red, alpha],
            ));
            Box::pin(async move { Ok(held.into_buffer()) })
        }
        // Not `ready`, which would hold a buffer's type, which crosses threads only held.
        Err(error) => Box::pin(async move { Err(error) }),
    }
}

/// A platform whose buffers the engine does not take yet: no hardware buffer.
#[cfg(not(target_vendor = "apple"))]
fn decoded(_size: [u32; 2], _pixels: Result<[u8; 4], ImageError>) -> ImageFuture<HardwareBuffer> {
    Box::pin(async move {
        Err(ImageError {
            kind: ImageErrorKind::Unsupported,
            message: "no hardware buffers on this system".to_owned(),
        })
    })
}

/// `buffer`'s size and its first pixel's bytes, BGRA, read while it is locked.
#[cfg(target_vendor = "apple")]
fn read(buffer: &HardwareBuffer) -> ([u32; 2], [u8; 4]) {
    use super::hardware_buffer::{HeldHardwareBuffer, pixel};

    let size = [buffer.width() as u32, buffer.height() as u32];
    (size, pixel(&HeldHardwareBuffer(buffer.clone()), 0, 0))
}

/// This shell decodes no hardware buffer on Linux or Windows (`decoded`), so none is read.
#[cfg(any(target_os = "linux", windows))]
fn read(buffer: &HardwareBuffer) -> ([u32; 2], [u8; 4]) {
    panic!(
        "the test shell decodes into no hardware buffer on this system, yet handed one of {:?}",
        buffer.size()
    )
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux", windows)))]
fn read(buffer: &HardwareBuffer) -> ([u32; 2], [u8; 4]) {
    match *buffer {}
}
