//! Where the programs' image codecs go: each opening, decoding and encoding to the main thread,
//! where Waft's codecs are (`state::image_codecs`), and each answer back in the program's terms
//! (`image_values`), a buffer the hardware codecs decoded as the platform's own
//! (`hardware_buffers`). The formats, and whether the system has hardware codecs, are read once as
//! WaOS starts, as they stay the same. A hardware buffer to encode from goes to Waft as the
//! platform's own buffer, on Apple's systems Waft's surface buffer over the program's `IOSurface`.
//!
//! The invariant: an image's calls reach the main thread after its opening, which answers before
//! the program holds the image, and its removal last, sent as the engine lets go of it; and a call
//! whose answer cannot come, as WaOS quits, is answered at once, so no program waits for it.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use waft::ui;
use waft::ui::image_codecs as codecs;
use waos::bindings::wasdk::platform::hardware_buffers::{ImageError, ImageErrorKind, PixelBuffer};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{
    HardwareBuffer, ImageFuture, OpenedImage, RunningProgram, ShellHardwareImageCodecs,
    ShellImageCodecs, ShellImageSource,
};

use super::hardware_buffers::{engine_buffer, waft_buffer};
use super::image_values::{ImageFacts, program_error, program_pixels, waft_pixels, waft_size};

/// The id the app gives each image a program opens, which its calls carry.
pub(crate) type ImageKey = u64;

/// One call of the programs' image codecs, on its way to the main thread, with where its answer
/// goes.
pub(crate) enum CodecCall {
    /// A program opens an image, kept under `image` once it opened.
    Open {
        image: ImageKey,
        source: ImageSource,
        answer: oneshot::Sender<Result<ImageFacts, codecs::ImageError>>,
    },
    /// A frame of one of the images, into the program's memory.
    DecodePixels {
        image: ImageKey,
        frame: u32,
        max_size: Option<ui::Size>,
        answer: oneshot::Sender<Result<codecs::PixelBuffer, codecs::ImageError>>,
    },
    /// A frame of one of the images, into a hardware buffer.
    DecodeHardware {
        image: ImageKey,
        frame: u32,
        max_size: Option<ui::Size>,
        answer: oneshot::Sender<Result<Arc<dyn ui::HardwareBuffer>, codecs::ImageError>>,
    },
    /// Pixels, or a hardware buffer, encoded.
    Encode {
        encoding: Encoding,
        format: String,
        quality: Option<f32>,
        answer: oneshot::Sender<Result<Vec<u8>, codecs::ImageError>>,
    },
    /// The program let one of its images go.
    Remove(ImageKey),
}

/// What an image is opened from.
pub(crate) enum ImageSource {
    Bytes(Vec<u8>),
    /// A file on the machine, inside the program's mounts.
    File(PathBuf),
}

/// What is encoded.
pub(crate) enum Encoding {
    Pixels(codecs::PixelBuffer),
    /// A hardware buffer, as Waft's of the platform's own.
    HardwareBuffer(Arc<dyn ui::HardwareBuffer>),
}

impl CodecCall {
    /// Answers `error`, whatever it asks.
    pub(crate) fn fail(self, error: codecs::ImageError) {
        // A program that ended meanwhile hears nothing.
        match self {
            CodecCall::Open { answer, .. } => drop(answer.send(Err(error))),
            CodecCall::DecodePixels { answer, .. } => drop(answer.send(Err(error))),
            CodecCall::DecodeHardware { answer, .. } => drop(answer.send(Err(error))),
            CodecCall::Encode { answer, .. } => drop(answer.send(Err(error))),
            CodecCall::Remove(_) => {}
        }
    }
}

/// The app's image codecs, as the engine reaches them: the formats as they were read, and every
/// call to the main thread.
pub(crate) struct CodecsOnRequest {
    calls: Calls,
    decoded_formats: Vec<String>,
    encoded_formats: Vec<String>,
    hardware: Option<Arc<HardwareOnRequest>>,
    last_image: AtomicU64,
}

impl CodecsOnRequest {
    /// The codecs, which decode and encode `decoded_formats` and `encoded_formats`, with hardware
    /// codecs where `hardware` says, and where their calls arrive on the main thread.
    pub(crate) fn new(
        decoded_formats: Vec<String>,
        encoded_formats: Vec<String>,
        hardware: bool,
    ) -> (CodecsOnRequest, UnboundedReceiver<CodecCall>) {
        let (calls, arriving) = mpsc::unbounded();
        let calls = Calls(calls);
        let codecs = CodecsOnRequest {
            hardware: hardware.then(|| Arc::new(HardwareOnRequest(calls.clone()))),
            calls,
            decoded_formats,
            encoded_formats,
            last_image: AtomicU64::default(),
        };
        (codecs, arriving)
    }

    /// The image `source` holds, opened on the main thread for `program`.
    fn open(&self, program: &RunningProgram, source: ImageSource) -> ImageFuture<OpenedImage> {
        tracing::info!(program = program.name(), "a program opens an image");
        let image = self.last_image.fetch_add(1, Ordering::Relaxed) + 1;
        let calls = self.calls.clone();
        let opening = self.calls.ask(move |answer| CodecCall::Open {
            image,
            source,
            answer,
        });
        Box::pin(async move {
            let facts = opening.await.map_err(program_error)?;
            Ok(OpenedImage {
                image: Box::new(ImageOnRequest { image, calls }),
                mime_type: facts.mime_type,
                size: facts.size,
                frames: facts.frames,
                orientation: facts.orientation,
                metadata: facts.metadata,
            })
        })
    }
}

impl ShellImageCodecs for CodecsOnRequest {
    fn decoded_formats(&self) -> Vec<String> {
        self.decoded_formats.clone()
    }

    fn encoded_formats(&self) -> Vec<String> {
        self.encoded_formats.clone()
    }

    fn open(&self, program: &RunningProgram, data: Vec<u8>) -> ImageFuture<OpenedImage> {
        CodecsOnRequest::open(self, program, ImageSource::Bytes(data))
    }

    fn open_file(
        &self,
        program: &RunningProgram,
        path: &std::path::Path,
    ) -> ImageFuture<OpenedImage> {
        CodecsOnRequest::open(self, program, ImageSource::File(path.to_owned()))
    }

    fn encode_pixels(
        &self,
        pixels: PixelBuffer,
        format: &str,
        quality: Option<f32>,
    ) -> ImageFuture<Vec<u8>> {
        let pixels = match waft_pixels(pixels) {
            Ok(pixels) => pixels,
            Err(error) => return Box::pin(std::future::ready(Err(error))),
        };
        self.calls.encode(Encoding::Pixels(pixels), format, quality)
    }

    fn hardware_image_codecs(&self) -> Option<Arc<dyn ShellHardwareImageCodecs>> {
        self.hardware
            .clone()
            .map(|hardware| hardware as Arc<dyn ShellHardwareImageCodecs>)
    }
}

/// The app's hardware codecs, as the engine reaches them: every call to the main thread.
struct HardwareOnRequest(Calls);

impl ShellHardwareImageCodecs for HardwareOnRequest {
    /// Waft's buffer, as the engine's: the platform's own behind it.
    fn decode(
        &self,
        image: &dyn ShellImageSource,
        frame: u32,
        max_size: Option<Size>,
    ) -> ImageFuture<HardwareBuffer> {
        let image: &dyn std::any::Any = image;
        let Some(image) = image.downcast_ref::<ImageOnRequest>() else {
            // Not `ready`, which would hold a buffer's type, which crosses threads only held.
            return Box::pin(
                async move { Err(unsupported("an image the app's codecs did not open")) },
            );
        };
        let decoding = self.0.ask(move |answer| CodecCall::DecodeHardware {
            image: image.image,
            frame,
            max_size: max_size.map(waft_size),
            answer,
        });
        Box::pin(async move {
            let buffer = decoding.await.map_err(program_error)?;
            engine_buffer(buffer.as_ref()).ok_or_else(|| {
                unsupported("the system's hardware buffer is of a kind WaOS does not hand out")
            })
        })
    }

    fn encode(
        &self,
        buffer: &HardwareBuffer,
        format: &str,
        quality: Option<f32>,
    ) -> ImageFuture<Vec<u8>> {
        self.0.encode(
            Encoding::HardwareBuffer(waft_buffer(buffer)),
            format,
            quality,
        )
    }
}

/// One image, as the engine holds it: its decodings go to the main thread, and its removal as it
/// drops.
struct ImageOnRequest {
    image: ImageKey,
    calls: Calls,
}

impl ShellImageSource for ImageOnRequest {
    fn decode_pixels(&self, frame: u32, max_size: Option<Size>) -> ImageFuture<PixelBuffer> {
        let decoding = self.calls.ask(move |answer| CodecCall::DecodePixels {
            image: self.image,
            frame,
            max_size: max_size.map(waft_size),
            answer,
        });
        Box::pin(async move { decoding.await.map(program_pixels).map_err(program_error) })
    }
}

impl Drop for ImageOnRequest {
    fn drop(&mut self) {
        self.calls.send(CodecCall::Remove(self.image));
    }
}

/// Where the calls go: the main thread.
#[derive(Clone)]
struct Calls(UnboundedSender<CodecCall>);

impl Calls {
    fn send(&self, call: CodecCall) {
        if let Err(going) = self.0.unbounded_send(call) {
            tracing::info!("WaOS is quitting: an image codec's call goes nowhere");
            going.into_inner().fail(quitting());
        }
    }

    /// Asks the call `call` makes of the answer it carries, and answers what comes back, or that
    /// WaOS is quitting where nothing will.
    fn ask<T: Send + 'static>(
        &self,
        call: impl FnOnce(oneshot::Sender<Result<T, codecs::ImageError>>) -> CodecCall,
    ) -> BoxFuture<'static, Result<T, codecs::ImageError>> {
        let (answer, answered) = oneshot::channel();
        self.send(call(answer));
        Box::pin(async move { answered.await.unwrap_or_else(|_| Err(quitting())) })
    }

    fn encode(
        &self,
        encoding: Encoding,
        format: &str,
        quality: Option<f32>,
    ) -> ImageFuture<Vec<u8>> {
        let format = format.to_owned();
        let encoded = self.ask(move |answer| CodecCall::Encode {
            encoding,
            format,
            quality,
            answer,
        });
        Box::pin(async move { encoded.await.map_err(program_error) })
    }
}

fn unsupported(message: &str) -> ImageError {
    ImageError {
        kind: ImageErrorKind::Unsupported,
        message: message.to_owned(),
    }
}

/// What a call answers once the main thread no longer takes calls.
fn quitting() -> codecs::ImageError {
    codecs::ImageError::new(codecs::ImageErrorKind::Other, "WaOS is quitting")
}
