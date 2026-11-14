//! The programs' image codecs on the main thread, over Waft's: each image opened on Waft's as the
//! engine asks, kept once it opened until the program lets it go, each frame decoded and each
//! encoding asked of Waft's codecs, and each answer sent back once Waft has it. The codecs are the
//! engine's shell's only where Waft's platform has them.
//!
//! The invariant: every call is answered once, with what Waft answered, or with a failure where
//! the image is not open here; and an image opened for a program that no longer waits for it is
//! let go at once, so none is kept that no program holds.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use waft::App;
use waft::ui::Platform;
use waft::ui::image_codecs as codecs;
use waos::ShellImageCodecs;

use crate::logic::image_codec_calls::{
    CodecCall, CodecsOnRequest, Encoding, ImageKey, ImageSource,
};
use crate::logic::image_values::{ImageFacts, program_facts};

/// Waft's image codecs, served to the engine as its shell's.
pub(crate) struct ImageCodecs {
    shell: Arc<CodecsOnRequest>,
    arriving: UnboundedReceiver<CodecCall>,
}

/// Waft's images the programs opened, by the app's key for each.
type OpenImages = Rc<RefCell<HashMap<ImageKey, Rc<dyn codecs::ImageSource>>>>;

impl ImageCodecs {
    /// Waft's image codecs on `platform`, to serve to the engine, their formats and whether they
    /// have hardware codecs read now; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<ImageCodecs> {
        let codecs = platform.image_codecs()?;
        let (shell, arriving) = CodecsOnRequest::new(
            codecs.decoded_formats(),
            codecs.encoded_formats(),
            codecs.hardware_image_codecs().is_some(),
        );
        Some(ImageCodecs {
            shell: Arc::new(shell),
            arriving,
        })
    }

    /// The codecs as the engine's shell has them: each call goes to the main thread, where
    /// [`serve`](Self::serve) carries it out.
    pub(crate) fn shell(&self) -> Arc<dyn ShellImageCodecs> {
        Arc::clone(&self.shell) as Arc<dyn ShellImageCodecs>
    }

    /// From now on, each image a program opens is one of Waft's, decoded and encoded as the
    /// program asks.
    pub(crate) fn serve(self, app: &mut App) {
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            let open = OpenImages::default();
            while let Some(call) = arriving.next().await {
                cx.update(|app| take(app, &open, call));
            }
        });
    }
}

/// Carries out `call`, from the engine.
fn take(app: &mut App, open: &OpenImages, call: CodecCall) {
    let platform = app.platform();
    let Some(codecs) = platform.image_codecs() else {
        call.fail(no_codecs());
        return;
    };
    match call {
        CodecCall::Open {
            image,
            source,
            answer,
        } => {
            let opening = match source {
                ImageSource::Bytes(data) => codecs.open(Arc::from(data)),
                ImageSource::File(path) => codecs.open_file(&path),
            };
            keep_once_opened(app, open, image, opening, answer);
        }
        CodecCall::DecodePixels {
            image,
            frame,
            max_size,
            answer,
        } => match opened(open, image) {
            Some(waft) => answer_once_done(app, waft.decode_pixels(frame, max_size), answer),
            None => drop(answer.send(Err(not_open()))),
        },
        CodecCall::DecodeHardware {
            image,
            frame,
            max_size,
            answer,
        } => match (opened(open, image), codecs.hardware_image_codecs()) {
            (Some(waft), Some(hardware)) => {
                let decoding = hardware.decode(waft.as_ref(), frame, max_size);
                answer_once_done(app, decoding, answer);
            }
            _ => drop(answer.send(Err(not_open()))),
        },
        CodecCall::Encode {
            encoding,
            format,
            quality,
            answer,
        } => {
            let encoding = match encoding {
                Encoding::Pixels(pixels) => codecs.encode_pixels(pixels, &format, quality),
                Encoding::HardwareBuffer(buffer) => match codecs.hardware_image_codecs() {
                    Some(hardware) => hardware.encode(buffer.as_ref(), &format, quality),
                    None => Box::pin(std::future::ready(Err(no_codecs()))),
                },
            };
            answer_once_done(app, encoding, answer);
        }
        CodecCall::Remove(image) => {
            open.borrow_mut().remove(&image);
        }
    }
}

/// Waft's image open under `image`, if it is.
fn opened(open: &OpenImages, image: ImageKey) -> Option<Rc<dyn codecs::ImageSource>> {
    open.borrow().get(&image).cloned()
}

/// Answers into `answer` what the image `opening` opens says of itself, once Waft has opened it,
/// keeping it under `image` while a program waits for it.
fn keep_once_opened(
    app: &mut App,
    open: &OpenImages,
    image: ImageKey,
    opening: codecs::ImageFuture<Box<dyn codecs::ImageSource>>,
    answer: oneshot::Sender<Result<ImageFacts, codecs::ImageError>>,
) {
    let open = Rc::clone(open);
    app.spawn(async move |_cx| match opening.await {
        Ok(waft) => {
            let facts = program_facts(waft.as_ref());
            open.borrow_mut().insert(image, Rc::from(waft));
            if answer.send(Ok(facts)).is_err() {
                open.borrow_mut().remove(&image);
            }
        }
        // A program that ended meanwhile hears nothing.
        Err(error) => drop(answer.send(Err(error))),
    });
}

/// Answers into `answer` what `doing` answers, once Waft has.
fn answer_once_done<T: 'static>(
    app: &mut App,
    doing: codecs::ImageFuture<T>,
    answer: oneshot::Sender<Result<T, codecs::ImageError>>,
) {
    app.spawn(async move |_cx| {
        // A program that ended meanwhile hears nothing.
        let _ = answer.send(doing.await);
    });
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
        ran.wait_for("/data/photo.jpg: image/heic 400x300, frames: still, upright");
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
                frame: 0,
                max_size: Some(ui::Size::new(16.0, 16.0)),
                hardware: false,
            }
        );
        assert!(
            asked.contains(&Asked::Decode {
                frame: 0,
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
            format!("{photo}: image/jpeg 32x48, frames: still, turned quarter-turn"),
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
            format!("bytes {photo}: image/jpeg 32x48, frames: still, turned quarter-turn"),
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
