//! A photo's file as a picture the window draws: opened by the system's image codecs, which read
//! the file themselves, and decoded upright at the size asked, by the hardware codecs into GPU
//! memory the renderer draws with no copy, or, where the system has none, into the program's
//! memory, which the renderer uploads.

use std::path::Path;
use std::sync::Arc;

use waft::ui::image_codecs::{ImageCodecs, ImageError, ImageFuture, PixelBuffer};
use waft::ui::{HardwareBuffer, Image};
use waft::{AsyncApp, Size};

/// The picture in the file at `path`, scaled down to fit `fitting`, in pixels, where given; none
/// where the system has no image codecs or the file is no picture they read, said in the log.
pub(crate) async fn picture_of(cx: &AsyncApp, path: &Path, fitting: Option<Size>) -> Option<Image> {
    let opening = codecs_call(cx, |codecs| codecs.open_file(path))?;
    let source = said(opening.await, path)?;
    let decoding = codecs_call(cx, |codecs| match codecs.hardware_image_codecs() {
        Some(hardware) => Decoding::Hardware(hardware.decode(source.as_ref(), 0, fitting)),
        None => Decoding::Pixels(source.decode_pixels(0, fitting)),
    })?;
    match decoding {
        Decoding::Hardware(decoding) => {
            let buffer = said(decoding.await, path)?;
            cx.update(|app| app.platform().import_hardware_buffer(buffer.as_ref()))
        }
        Decoding::Pixels(decoding) => {
            let pixels = said(decoding.await, path)?;
            cx.update(|app| app.platform().import_pixels(pixels))
        }
    }
}

/// What a picture's decoding answers, by the memory it decodes into.
enum Decoding {
    Hardware(ImageFuture<Arc<dyn HardwareBuffer>>),
    Pixels(ImageFuture<PixelBuffer>),
}

/// What `call` asks of the system's image codecs; none where the system has none.
fn codecs_call<R>(cx: &AsyncApp, call: impl FnOnce(&dyn ImageCodecs) -> R) -> Option<R> {
    cx.update(|app| {
        let platform = app.platform();
        Some(call(platform.image_codecs()?))
    })
}

/// `answer`'s value; none where it failed, which the log says of the file at `path`.
fn said<T>(answer: Result<T, ImageError>, path: &Path) -> Option<T> {
    answer
        .inspect_err(|error| tracing::warn!("{} is not shown: {error}", path.display()))
        .ok()
}
