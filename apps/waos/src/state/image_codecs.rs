//! The programs' image codecs, over Waft's: the engine opens, decodes and encodes from a
//! program's thread; each call is posted to the App's thread (`app_thread_bound`), where the image
//! is opened on Waft's codecs, kept once it opened until the program lets it go, and decoded and
//! encoded as asked, its answer sent back once Waft has it, in the program's terms
//! (`logic::image_values`), a buffer the hardware codecs decoded as the platform's own
//! (`logic::hardware_buffers`). The formats, and whether the system has hardware codecs, are read
//! once as WaOS starts, as they stay the same. A hardware buffer to encode from goes to Waft as the
//! platform's own buffer, on Apple's systems Waft's surface buffer over the program's `IOSurface`.
//! The codecs are the engine's shell's only where Waft's platform has them.
//!
//! The invariant: an image's calls reach the App's thread after its opening, which answers the
//! image the engine then holds, and its removal last, posted as the engine lets go of it, or as
//! its opening's answer is dropped unheard, so none is kept that no program holds; and every call
//! is answered once, with what Waft answered, with a failure where the image is not open there, or
//! at once as WaOS quits, so no program waits for an answer that cannot come.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::FutureExt;
use waft::App;
use waft::ui::image_codecs as codecs;
use waft::ui::{Platform, PlatformRef};
use waos::bindings::wasdk::platform::hardware_buffers::{ImageError, ImageErrorKind, PixelBuffer};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{
    HardwareBuffer, ImageFuture, OpenedImage, RunningProgram, ShellHardwareImageCodecs,
    ShellImageCodecs, ShellImageSource,
};

use super::app_thread_bound::{AppThreadBound, AppThreadCalls};
use crate::logic::hardware_buffers::{engine_buffer, waft_buffer};
use crate::logic::image_values::{
    program_error, program_facts, program_pixels, waft_pixels, waft_size,
};

/// The id the app gives each image a program opens, which its calls carry.
type ImageKey = u64;

/// Waft's image codecs, served to the engine as its shell's.
pub(crate) struct ImageCodecs {
    shell: Arc<CodecsOnRequest>,
    calls: AppThreadCalls<OpenImages>,
}

impl ImageCodecs {
    /// Waft's image codecs on `platform`, to serve to the engine, their formats and whether they
    /// have hardware codecs read now; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<ImageCodecs> {
        let codecs = platform.image_codecs()?;
        let (images, calls) = AppThreadBound::new();
        let hardware = codecs.hardware_image_codecs().is_some();
        let shell = CodecsOnRequest {
            hardware: hardware.then(|| Arc::new(HardwareOnRequest(images.clone()))),
            images,
            decoded_formats: codecs.decoded_formats(),
            encoded_formats: codecs.encoded_formats(),
            last_image: AtomicU64::default(),
        };
        Some(ImageCodecs {
            shell: Arc::new(shell),
            calls,
        })
    }

    /// The codecs as the engine's shell has them: each call is posted to the App's thread, where
    /// [`serve`](Self::serve) runs it.
    pub(crate) fn shell(&self) -> Arc<dyn ShellImageCodecs> {
        Arc::clone(&self.shell) as Arc<dyn ShellImageCodecs>
    }

    /// From now on, each image a program opens is one of Waft's, decoded and encoded as the
    /// program asks.
    pub(crate) fn serve(self, app: &mut App) {
        let images = OpenImages {
            platform: app.platform(),
            open: Rc::default(),
        };
        self.calls.serve(app, images);
    }
}

/// The app's image codecs, as the engine reaches them: the formats as they were read, and every
/// call posted to the App's thread.
struct CodecsOnRequest {
    images: AppThreadBound<OpenImages>,
    decoded_formats: Vec<String>,
    encoded_formats: Vec<String>,
    hardware: Option<Arc<HardwareOnRequest>>,
    last_image: AtomicU64,
}

/// What an image is opened from.
enum ImageSource {
    Bytes(Vec<u8>),
    /// A file on the machine, inside the program's mounts.
    File(PathBuf),
}

impl CodecsOnRequest {
    /// The image `source` holds, opened on the App's thread for `program`.
    fn open(&self, program: &RunningProgram, source: ImageSource) -> ImageFuture<OpenedImage> {
        tracing::info!(program = program.name(), "a program opens an image");
        let image = self.last_image.fetch_add(1, Ordering::Relaxed) + 1;
        let handle = self.images.clone();
        ask(&self.images, move |images| {
            images.open(image, source, handle)
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

    fn open_file(&self, program: &RunningProgram, path: &Path) -> ImageFuture<OpenedImage> {
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
        let format = format.to_owned();
        ask(&self.images, move |images| {
            let encoding = images
                .codecs()
                .map(|codecs| codecs.encode_pixels(pixels, &format, quality));
            async move { encoding.ok_or_else(no_codecs)?.await }
        })
    }

    fn hardware_image_codecs(&self) -> Option<Arc<dyn ShellHardwareImageCodecs>> {
        self.hardware
            .clone()
            .map(|hardware| hardware as Arc<dyn ShellHardwareImageCodecs>)
    }
}

/// The app's hardware codecs, as the engine reaches them: every call posted to the App's thread.
struct HardwareOnRequest(AppThreadBound<OpenImages>);

impl ShellHardwareImageCodecs for HardwareOnRequest {
    /// Waft's buffer, as the engine's: the platform's own behind it.
    fn decode(
        &self,
        image: &dyn ShellImageSource,
        index: u32,
        max_size: Option<Size>,
    ) -> ImageFuture<HardwareBuffer> {
        let image: &dyn std::any::Any = image;
        let Some(image) = image.downcast_ref::<ImageOnRequest>() else {
            // Not `ready`, which would hold a buffer's type, which crosses threads only held.
            return Box::pin(
                async move { Err(unsupported("an image the app's codecs did not open")) },
            );
        };
        let image = image.image;
        let max_size = max_size.map(waft_size);
        let decoding = ask(&self.0, move |images| {
            let decoding = images
                .opened(image)
                .zip(images.hardware())
                .map(|(waft, hardware)| hardware.decode(waft.as_ref(), index, max_size));
            async move { decoding.ok_or_else(not_open)?.await }
        });
        Box::pin(async move {
            let buffer = decoding.await?;
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
        let Some(waft) = waft_buffer(buffer) else {
            return Box::pin(std::future::ready(Err(unsupported(
                "offscreen hardware buffers cannot be encoded by the platform hardware codec",
            ))));
        };
        let format = format.to_owned();
        ask(&self.0, move |images| {
            let encoding = images
                .hardware()
                .map(|hardware| hardware.encode(waft.as_ref(), &format, quality));
            async move { encoding.ok_or_else(no_codecs)?.await }
        })
    }
}

/// One image, as the engine holds it: its decodings are posted to the App's thread, and its
/// removal as it drops.
struct ImageOnRequest {
    image: ImageKey,
    images: AppThreadBound<OpenImages>,
}

impl ShellImageSource for ImageOnRequest {
    fn decode_pixels(&self, index: u32, max_size: Option<Size>) -> ImageFuture<PixelBuffer> {
        let image = self.image;
        let max_size = max_size.map(waft_size);
        ask(&self.images, move |images| {
            let decoding = images
                .opened(image)
                .map(|waft| waft.decode_pixels(index, max_size));
            async move { decoding.ok_or_else(not_open)?.await.map(program_pixels) }
        })
    }
}

impl Drop for ImageOnRequest {
    fn drop(&mut self) {
        let image = self.image;
        let _ = self.images.post(move |images| {
            images.open.borrow_mut().remove(&image);
        });
    }
}

/// Asks `call` of the open images on the App's thread, and answers what the future it starts
/// there resolves to, in the program's terms, or that WaOS is quitting where nothing will.
fn ask<A: Send + 'static, F: Future<Output = Result<A, codecs::ImageError>> + 'static>(
    images: &AppThreadBound<OpenImages>,
    call: impl FnOnce(&mut OpenImages) -> F + Send + 'static,
) -> ImageFuture<A> {
    let answered = images.ask(call);
    Box::pin(answered.map(|answer| match answer {
        Ok(answer) => answer.map_err(program_error),
        Err(_) => Err(program_error(quitting())),
    }))
}

/// Waft's images the programs opened, by the app's key for each, on the App's thread; shared with
/// the openings, which keep an image once Waft has opened it.
struct OpenImages {
    platform: PlatformRef,
    open: Rc<RefCell<HashMap<ImageKey, Rc<dyn codecs::ImageSource>>>>,
}

impl OpenImages {
    fn codecs(&self) -> Option<&dyn codecs::ImageCodecs> {
        self.platform.image_codecs()
    }

    fn hardware(&self) -> Option<&dyn codecs::HardwareImageCodecs> {
        self.codecs()?.hardware_image_codecs()
    }

    /// Waft's image open under `image`, if it is.
    fn opened(&self, image: ImageKey) -> Option<Rc<dyn codecs::ImageSource>> {
        self.open.borrow().get(&image).cloned()
    }

    /// Opens on Waft's codecs the image `source` holds, kept under `image` once it opened, and
    /// answers it as the engine holds it, through `images`; dropped unheard, it lets the image go.
    fn open(
        &mut self,
        image: ImageKey,
        source: ImageSource,
        images: AppThreadBound<OpenImages>,
    ) -> impl Future<Output = Result<OpenedImage, codecs::ImageError>> + use<> {
        let opening = self.codecs().map(|codecs| match source {
            ImageSource::Bytes(data) => codecs.open(Arc::from(data)),
            ImageSource::File(path) => codecs.open_file(&path),
        });
        let open = Rc::clone(&self.open);
        async move {
            let waft = opening.ok_or_else(no_codecs)?.await?;
            let facts = program_facts(waft.as_ref());
            open.borrow_mut().insert(image, Rc::from(waft));
            Ok(OpenedImage {
                image: Box::new(ImageOnRequest { image, images }),
                mime_type: facts.mime_type,
                size: facts.size,
                pictures: facts.pictures,
                orientation: facts.orientation,
                metadata: facts.metadata,
            })
        }
    }
}

fn unsupported(message: &str) -> ImageError {
    ImageError {
        kind: ImageErrorKind::Unsupported,
        message: message.to_owned(),
    }
}

/// What a call answers once the App's thread no longer takes calls.
fn quitting() -> codecs::ImageError {
    codecs::ImageError::new(codecs::ImageErrorKind::Other, "WaOS is quitting")
}

fn not_open() -> codecs::ImageError {
    codecs::ImageError::new(
        codecs::ImageErrorKind::Other,
        "the image is not open in WaOS",
    )
}

fn no_codecs() -> codecs::ImageError {
    codecs::ImageError::new(
        codecs::ImageErrorKind::Unsupported,
        "the system has no such image codecs",
    )
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;

    use waft::ui;
    use waft_test::{CodecCall as Asked, ImageAnswers, ImageFacts, MockImageCodecs, TestPlatform};
    use waos::ProgramEnd;
    use waos_package::ProgramId;

    use super::*;
    use crate::state::test_system::{Lines, TestSystem, real_path};

    const IMAGE_CODECS: &str = "dev.example.imagecodecs";

    fn answers() -> ImageAnswers {
        ImageAnswers {
            decoded_formats: vec!["image/jpeg".to_owned(), "image/heic".to_owned()],
            encoded_formats: vec!["image/png".to_owned()],
            opened: Ok(ImageFacts::still("image/heic", ui::Size::new(400.0, 300.0))),
            pixels: Ok([0, 0, 255, 255]),
            encoded: Ok(b"encoded".to_vec()),
        }
    }

    /// The system whose platform's codecs are `codecs`, running `examples/image-codecs` on the
    /// sources `sources` names, beside a photo of `photo`'s bytes at `/data/photo.jpg`.
    fn running(codecs: Rc<dyn codecs::ImageCodecs>, sources: &str, photo: &[u8]) -> Ran {
        let platform = TestPlatform::new().with_image_codecs(codecs);
        let system = TestSystem::new(platform);
        let id: ProgramId = IMAGE_CODECS.parse().unwrap();
        let data = system.store.folders(&id).unwrap().data;
        std::fs::write(data.join("sources"), sources).unwrap();
        std::fs::write(data.join("photo.jpg"), photo).unwrap();
        let lines = Arc::new(Lines::default());
        let running = system.run_as(waos_test_programs::IMAGE_CODECS, IMAGE_CODECS, &lines);
        Ran {
            system,
            lines,
            running,
            data,
        }
    }

    struct Ran {
        system: TestSystem,
        lines: Arc<Lines>,
        running: waos::RunningProgram,
        data: std::path::PathBuf,
    }

    impl Ran {
        /// Runs the app's tasks until the program said `line`.
        fn wait_for(&self, line: &str) {
            self.system.run_tasks_until(line, || self.lines.has(line));
        }

        /// Runs the app's tasks until the program ended, which it does once the app answered
        /// what it asked.
        fn ends(&self) {
            use futures::FutureExt;

            let ended = || self.running.ended().now_or_never();
            self.system
                .run_tasks_until("the program ends", || ended().is_some());
            let end = ended().expect("ended");
            assert!(
                matches!(end, ProgramEnd::Returned(Ok(()))),
                "{end:?}, having said {:#?}",
                self.lines.all()
            );
        }
    }

    #[test]
    fn a_programs_images_are_wafts_and_what_waft_answers_reaches_the_program() {
        let codecs = MockImageCodecs::with_hardware(answers());
        let ran = running(Rc::new(codecs.clone()), "/data/photo.jpg\n", b"a photo");
        ran.wait_for("formats: decodes image/jpeg, image/heic; encodes image/png");
        ran.wait_for("/data/photo.jpg: image/heic 400x300, pictures: still, upright");
        ran.wait_for("/data/photo.jpg: location none");
        ran.wait_for("/data/photo.jpg: pixels 16x12 rgba8 premultiplied, all blue");
        ran.wait_for("/data/photo.jpg: encoded image/png opens 400x300");
        ran.wait_for(
            "/data/photo.jpg: hardware buffer failed (unsupported: the system's hardware buffer is of a kind WaOS does not hand out)",
        );
        ran.ends();
        let photo = real_path(&ran.data).join("photo.jpg");
        let asked = codecs.asked();
        assert_eq!(
            asked[0],
            Asked::OpenFile(photo),
            "where the photo is on the machine"
        );
        assert_eq!(
            asked[1],
            Asked::Decode {
                index: 0,
                max_size: Some(ui::Size::new(16.0, 16.0)),
                hardware: false,
            }
        );
        assert!(
            asked.contains(&Asked::Decode {
                index: 0,
                max_size: None,
                hardware: true,
            }),
            "{asked:?}"
        );
    }

    #[test]
    fn a_platform_without_image_codecs_gives_the_engine_none() {
        assert!(ImageCodecs::of(&TestPlatform::new()).is_none());
        let codecs = Rc::new(MockImageCodecs::new(answers()));
        assert!(ImageCodecs::of(&TestPlatform::new().with_image_codecs(codecs)).is_some());
    }

    /// A real photo, `tests/media/photo.jpg`, through Image I/O: 48 by 32 as stored, its left half
    /// red and its right half blue, with EXIF saying to turn it a quarter clockwise, so 32 by 48
    /// upright, red above and blue below, with EXIF's common fields and a location.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_real_photo_decodes_and_encodes_through_image_io() {
        let codecs = waft_embedder_winit::test_support::image_codecs().expect("Image I/O");
        let photo = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/media/photo.jpg"
        ))
        .expect("the photo");
        let ran = running(codecs, "/data/photo.jpg\nbytes /data/photo.jpg\n", &photo);
        let photo = "/data/photo.jpg";
        for line in [
            format!("{photo}: image/jpeg 32x48, pictures: still, turned quarter-turn"),
            format!("{photo}: taken 2024-05-01 10:20:30 +02:00"),
            format!("{photo}: camera Waft Test Camera, lens Test Lens 4.2mm f/1.8"),
            format!(
                "{photo}: exposure 1/120 s, f/1.8, ISO 200, 4.2 mm (26 mm equivalent), -0.33 EV, flash fired"
            ),
            format!("{photo}: location 48.85083, -2.29500, 35 m"),
            format!(
                "{photo}: hardware buffer 32x48 for gpu-sampled, composer-overlay, cpu-read, read 32x48 bgra8 premultiplied, red above blue"
            ),
            format!("{photo}: hardware encoded image/jpeg opens 32x48"),
            format!("bytes {photo}: image/jpeg 32x48, pictures: still, turned quarter-turn"),
        ] {
            ran.wait_for(&line);
        }
        let said = ran.lines.all();
        let pixels = said
            .iter()
            .find(|line| line.starts_with(&format!("{photo}: pixels ")))
            .expect("its pixels");
        assert!(
            pixels.ends_with("x16 rgba8 premultiplied, red above blue"),
            "fitting 16 by 16, upright: {pixels}"
        );
        assert!(
            said.iter()
                .any(|line| line.starts_with(&format!("{photo}: encoded image/png opens "))),
            "{said:?}"
        );
        ran.ends();
    }
}
