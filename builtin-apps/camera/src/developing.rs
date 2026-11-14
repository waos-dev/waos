//! A photo developed: the photo the system took and encoded, drawn through the look it was taken
//! in and encoded again. The system's hardware codecs decode it upright into GPU memory, the
//! painter samples that through a texture and draws it onto a sheet of the photo's size, and the
//! hardware codecs encode the sheet, so the photo's pixels never cross the program's memory. A
//! photo taken in the original look is kept as the system encoded it, byte for byte, with what the
//! camera wrote of how it was taken; a developed one carries none of that, since the system's
//! codecs write no metadata.

use std::sync::Arc;

use waft::ui::image_codecs::{HardwareImageCodecs, ImageCodecs, ImageError, ImageFuture};
use waft::ui::{HardwareBuffer, Photo};
use waft::{App, AsyncApp, Entity};

use crate::darkroom::Darkroom;
use crate::look_painter::{LookPainter, Shown};
use crate::looks::Look;
use crate::sheet::Sheet;

/// The formats a developed photo is kept in, the first the system encodes, each with what its
/// files' names end in.
const KEPT_FORMATS: [(&str, &str); 2] = [("image/jpeg", "jpg"), ("image/png", "png")];

/// How good a developed photo's encoding is, from 0 to 1, where its format loses detail.
const QUALITY: f32 = 0.92;

/// A photo ready to keep.
pub(crate) struct Developed {
    pub(crate) encoded: Vec<u8>,
    /// What a file of it is named to end in.
    pub(crate) extension: String,
}

/// `photo` as it is kept when taken in `look`; why not, worded for the user.
pub(crate) async fn developed(
    cx: &AsyncApp,
    darkroom: &Entity<Darkroom>,
    photo: Photo,
    look: Look,
) -> Result<Developed, String> {
    if look == Look::Original {
        return Ok(Developed {
            extension: extension_of(&photo.mime_type),
            encoded: photo.bytes,
        });
    }
    let painter = cx
        .update(|app| darkroom.read(app).painter())
        .ok_or("The app has no WebGPU device to draw the look with.")?;
    let opening = codecs_call(cx, |codecs| codecs.open(Arc::from(photo.bytes)))?;
    let source = opening.await.map_err(failure_words)?;
    let decoding = hardware_call(cx, |hardware| hardware.decode(source.as_ref(), 0, None))?;
    let taken = decoding.await.map_err(failure_words)?;
    let (encoding, extension) = cx.update(|app| {
        let sheet = drawn(app, &painter, &*taken, look)?;
        encoding_of(app, &sheet)
    })?;
    let encoded = encoding.await.map_err(failure_words)?;
    Ok(Developed {
        encoded,
        extension: extension.to_owned(),
    })
}

/// `taken`, drawn through `look` onto a sheet of its size.
fn drawn(
    app: &App,
    painter: &LookPainter,
    taken: &dyn HardwareBuffer,
    look: Look,
) -> Result<Sheet, String> {
    let platform = app.platform();
    let picture = platform
        .import_texture(taken, painter.device())
        .ok_or("The app's device cannot sample the photo.")?;
    let size = [taken.width(), taken.height()];
    let sheet = Sheet::to_encode(&*platform, painter.device(), size)
        .map_err(|why| format!("The system gave no memory to draw the photo in: {why}."))?;
    painter.paint(&picture, &sheet.texture, look.grade(), Shown::default());
    Ok(sheet)
}

/// The hardware codecs' encoding of `sheet` under way, in the format a developed photo is kept
/// in, and what a file of it is named to end in.
fn encoding_of(app: &App, sheet: &Sheet) -> Result<(ImageFuture<Vec<u8>>, &'static str), String> {
    let platform = app.platform();
    let codecs = platform.image_codecs().ok_or(NO_CODECS)?;
    let (format, extension) = kept_format(codecs).ok_or("The system encodes no JPEG or PNG.")?;
    let hardware = codecs.hardware_image_codecs().ok_or(NO_HARDWARE_CODECS)?;
    let encoding = hardware.encode(sheet.buffer.as_ref(), format, Some(QUALITY));
    Ok((encoding, extension))
}

const NO_CODECS: &str = "This system has no image codecs to develop a photo with.";
const NO_HARDWARE_CODECS: &str =
    "This system's image codecs do not reach GPU memory, which a look is drawn in.";

/// The format a developed photo is kept in: the first of [`KEPT_FORMATS`] the system encodes.
fn kept_format(codecs: &dyn ImageCodecs) -> Option<(&'static str, &'static str)> {
    let encoded = codecs.encoded_formats();
    KEPT_FORMATS
        .into_iter()
        .find(|(format, _)| encoded.iter().any(|written| written == format))
}

/// What a file of the format `mime_type` names is named to end in: the type's own name, and
/// `jpg` for a JPEG.
fn extension_of(mime_type: &str) -> String {
    let subtype = mime_type.rsplit('/').next().unwrap_or_default();
    match subtype {
        "jpeg" | "" => "jpg".to_owned(),
        other => other.to_owned(),
    }
}

/// What `call` asks of the system's image codecs.
fn codecs_call<R>(cx: &AsyncApp, call: impl FnOnce(&dyn ImageCodecs) -> R) -> Result<R, String> {
    cx.update(|app| {
        let platform = app.platform();
        Ok(call(platform.image_codecs().ok_or(NO_CODECS)?))
    })
}

/// What `call` asks of the system's hardware image codecs.
fn hardware_call<R>(
    cx: &AsyncApp,
    call: impl FnOnce(&dyn HardwareImageCodecs) -> R,
) -> Result<R, String> {
    codecs_call(cx, |codecs| {
        Ok(call(
            codecs.hardware_image_codecs().ok_or(NO_HARDWARE_CODECS)?,
        ))
    })?
}

fn failure_words(error: ImageError) -> String {
    format!("The photo could not be developed: {error}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_photos_file_ends_in_its_formats_name() {
        assert_eq!(extension_of("image/jpeg"), "jpg");
        assert_eq!(extension_of("image/heic"), "heic");
        assert_eq!(extension_of("image/png"), "png");
        assert_eq!(extension_of(""), "jpg");
    }
}
