//! The library's thumbnails, kept in the program's own `/cache/thumbnails` so that a later run
//! draws each without reading its video again. A thumbnail is the frame the system read of a video,
//! whose hardware buffer the system's hardware codecs encode straight from it, with no copy through
//! the program's memory, as HEIC where they encode it, a thumbnail in a few kilobytes, and PNG
//! otherwise; and decoded back by them into a hardware buffer of the program's own, which holds no
//! decoder's frame, and which the app draws as an image with no copy. Where the system has image
//! codecs but no hardware codecs, a thumbnail goes through the program's memory instead, its pixels
//! read back to be encoded and decoded into the program's memory to be drawn, which the log says
//! once.
//!
//! A thumbnail is kept by its video's URL and its file's stamp, so a changed file's is read anew;
//! the one it replaces stays until the system clears the program's cache.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use waft_embedder::image_codecs::{ImageCodecs, ImageError, ImageFuture};
use waft_embedder::{HardwareBuffer, Image, VideoFrame};
use waft_foundation::{App, AsyncApp};

/// Where the thumbnails are kept between runs.
const THUMBNAILS: &str = "/cache/thumbnails";

/// The formats a thumbnail is kept in, the first the system encodes.
const KEPT_FORMATS: [&str; 2] = ["image/heic", "image/png"];

/// Whether the log said yet that the system has no hardware codecs.
static SAID_NO_HARDWARE: AtomicBool = AtomicBool::new(false);

/// Where the thumbnail of the video at `url`, whose file has `stamp`, is kept.
pub(crate) fn path_of(url: &str, stamp: &str) -> PathBuf {
    PathBuf::from(THUMBNAILS).join(format!("{:016x}", fnv1a(&format!("{url}\t{stamp}"))))
}

/// The thumbnail kept at `path`, decoded and imported as an image to draw; none where none is
/// kept, the system has no image codecs, or it could not be decoded, said so.
pub(crate) async fn kept(cx: &AsyncApp, path: PathBuf) -> Option<Image> {
    if !path.is_file() {
        return None;
    }
    let opening = codecs_call(cx, |codecs| codecs.open_file(&path))?;
    let image = said(opening.await, "read")?;
    let decoding = codecs_call(cx, |codecs| match codecs.hardware_image_codecs() {
        Some(hardware) => Decoding::Hardware(hardware.decode(image.as_ref(), 0, None)),
        None => {
            say_no_hardware();
            Decoding::Pixels(image.decode_pixels(0, None))
        }
    })?;
    match decoding {
        Decoding::Hardware(decoding) => {
            let buffer = said(decoding.await, "decoded")?;
            cx.update(|app| app.platform().import_hardware_buffer(buffer.as_ref()))
        }
        Decoding::Pixels(decoding) => {
            let pixels = said(decoding.await, "decoded")?;
            cx.update(|app| app.platform().import_pixels(pixels))
        }
    }
}

/// Keeps `frame`, a video's thumbnail the system read, at `path`, encoded from its hardware buffer,
/// where the system has image codecs; a failure is said and nothing kept.
pub(crate) async fn keep(cx: &AsyncApp, frame: Arc<dyn VideoFrame>, path: PathBuf) {
    let encoding = codecs_call(cx, |codecs| {
        let format = kept_format(codecs)?;
        Some(match codecs.hardware_image_codecs() {
            Some(hardware) => {
                let buffer = frame.hardware_buffer();
                Encoding::Hardware(hardware.encode(buffer.as_ref(), format, None))
            }
            None => {
                say_no_hardware();
                Encoding::Pixels(frame.read_pixels(), format)
            }
        })
    })
    .flatten();
    let encoded = match encoding {
        Some(Encoding::Hardware(encoding)) => encoding.await,
        Some(Encoding::Pixels(reading, format)) => match reading.await {
            Ok(pixels) => {
                match codecs_call(cx, |codecs| codecs.encode_pixels(pixels, format, None)) {
                    Some(encoding) => encoding.await,
                    None => return,
                }
            }
            Err(error) => Err(error),
        },
        None => return,
    };
    let Some(encoded) = said(encoded, "encoded") else {
        return;
    };
    let written = std::fs::create_dir_all(THUMBNAILS).and_then(|()| std::fs::write(&path, encoded));
    if let Err(error) = written {
        eprintln!("a video's thumbnail is not kept: {error}");
    }
}

/// The format a thumbnail is kept in: the first of [`KEPT_FORMATS`] the system encodes.
fn kept_format(codecs: &dyn ImageCodecs) -> Option<&'static str> {
    let encoded = codecs.encoded_formats();
    let format = KEPT_FORMATS
        .into_iter()
        .find(|format| encoded.iter().any(|written| written == format));
    if format.is_none() {
        eprintln!("the system encodes neither HEIC nor PNG: thumbnails are not kept");
    }
    format
}

/// What a thumbnail's decoding answers, by the memory it decodes into.
enum Decoding {
    Hardware(ImageFuture<Arc<dyn HardwareBuffer>>),
    Pixels(ImageFuture<waft_embedder::image_codecs::PixelBuffer>),
}

/// What a thumbnail's encoding answers, by the memory it encodes from: the pixels to read back
/// first, with the format.
enum Encoding {
    Hardware(ImageFuture<Vec<u8>>),
    Pixels(
        ImageFuture<waft_embedder::image_codecs::PixelBuffer>,
        &'static str,
    ),
}

/// What `call` asks of the platform's image codecs; none where the system has none.
fn codecs_call<R>(cx: &AsyncApp, call: impl FnOnce(&dyn ImageCodecs) -> R) -> Option<R> {
    cx.update(|app: &mut App| {
        let platform = app.platform();
        Some(call(platform.image_codecs()?))
    })
}

/// `answer`'s value; none where it failed, said with what was not `done` to the thumbnail.
fn said<T>(answer: Result<T, ImageError>, done: &str) -> Option<T> {
    answer
        .inspect_err(|error| eprintln!("a kept thumbnail is not {done}: {}", error.message))
        .ok()
}

/// Says, once, that thumbnails go through the program's memory.
fn say_no_hardware() {
    if !SAID_NO_HARDWARE.swap(true, Ordering::Relaxed) {
        eprintln!(
            "the system has no hardware image codecs: thumbnails are encoded and decoded in the program's memory"
        );
    }
}

/// FNV-1a's 64-bit hash of `text`: a name for a file that stays the same across runs and builds.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumbnail_is_kept_by_its_videos_url_and_stamp() {
        let kept = path_of("file:///data/clip.mp4", "6-1700000000");
        assert_eq!(kept.parent(), Some(std::path::Path::new(THUMBNAILS)));
        assert_eq!(kept, path_of("file:///data/clip.mp4", "6-1700000000"));
        assert_ne!(
            kept,
            path_of("file:///data/clip.mp4", "7-1700000001"),
            "changed"
        );
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
    }
}
