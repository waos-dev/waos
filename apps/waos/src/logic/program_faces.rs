//! A program as its windows show it where the system shows a window's own title and icon, as
//! Android's Recents shows a task's: the name its user knows it by (`program_name`) and its
//! package's icon, a PNG, as the four bytes a pixel Waft's windows take; and, where its windows are
//! an app of their own, on Windows and Linux, an installed program's identity
//! (`program_identities`).
//!
//! The invariant: a program whose icon cannot be read still gets its windows, with the app's
//! icon in its place.

use std::io::Cursor;
use std::path::Path;

use waft::ui::IconImage;
use waos_package::ProgramId;
use waos_program_window::{ProgramFace, ProgramIdentity};
use waos_store::Store;

use super::program_identities::{Identities, installed_app_id};
use super::program_name::name_of;

/// The faces of the programs, as the store has them, with their identities where the system's
/// windows take them.
#[derive(Clone)]
pub(crate) struct ProgramFaces {
    store: Store,
    /// What the programs' identities are made of; none where a window is no app of its own.
    identities: Option<Identities>,
}

impl ProgramFaces {
    /// The faces of the programs in `store`, the installed ones' with identities made of
    /// `identities`, where there are any.
    pub(crate) fn new(store: Store, identities: Option<Identities>) -> ProgramFaces {
        ProgramFaces { store, identities }
    }

    /// The face of the program that runs as `runs_as`, with its identity where it is installed.
    pub(crate) fn face(&self, runs_as: &str) -> ProgramFace {
        let face = program_face(&self.store, runs_as);
        ProgramFace {
            identity: self.identity_of(runs_as, &face.name),
            ..face
        }
    }

    /// The identity of the program that runs as `runs_as`, which its user knows as `name`; none
    /// for a program not installed.
    fn identity_of(&self, runs_as: &str, name: &str) -> Option<ProgramIdentity> {
        let identities = self.identities.as_ref()?;
        let (id, app_id) = installed_app_id(&self.store, runs_as)?;
        Some(identities.identity_of(&id, app_id, name))
    }
}

/// The face of the program that runs as `runs_as`: its name in `store`, and its icon there where
/// it is installed with one that reads; no identity, as on a system whose windows are no apps of
/// their own.
pub(crate) fn program_face(store: &Store, runs_as: &str) -> ProgramFace {
    let icon = runs_as
        .parse::<ProgramId>()
        .ok()
        .and_then(|id| store.icon(&id))
        .and_then(|path| icon_at(&path));
    ProgramFace {
        name: name_of(store, runs_as),
        icon,
        identity: None,
    }
}

/// The icon in the PNG at `path`; none where it cannot be read, which is said.
fn icon_at(path: &Path) -> Option<IconImage> {
    let decoded = std::fs::read(path)
        .map_err(|error| error.to_string())
        .and_then(|png| icon_of(&png));
    match decoded {
        Ok(icon) => Some(icon),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "a program's icon cannot be read");
            None
        }
    }
}

/// The icon `png` is, as eight-bit RGBA, whatever its own colours and depth.
pub(crate) fn icon_of(png: &[u8]) -> Result<IconImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let size = reader
        .output_buffer_size()
        .ok_or("the picture is too large")?;
    let mut pixels = vec![0; size];
    let frame = reader
        .next_frame(&mut pixels)
        .map_err(|error| error.to_string())?;
    pixels.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|[grey, alpha]| [*grey, *grey, *grey, *alpha])
            .collect(),
        other => return Err(format!("its colours, {other:?}, did not become RGBA")),
    };
    Ok(IconImage {
        width: frame.width,
        height: frame.height,
        rgba: rgba.into(),
        template: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG `width` by `height` of `color`, in `color_type`'s channels.
    fn png_of(width: u32, height: u32, color_type: png::ColorType, pixel: &[u8]) -> Vec<u8> {
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&pixel.repeat((width * height) as usize))
            .unwrap();
        writer.finish().unwrap();
        png
    }

    #[test]
    fn an_rgba_icon_is_its_own_pixels() {
        let icon = icon_of(&png_of(2, 3, png::ColorType::Rgba, &[1, 2, 3, 4])).unwrap();
        assert_eq!((icon.width, icon.height), (2, 3));
        assert_eq!(&*icon.rgba, [1, 2, 3, 4].repeat(6).as_slice());
    }

    #[test]
    fn an_icon_without_alpha_is_opaque() {
        let icon = icon_of(&png_of(1, 1, png::ColorType::Rgb, &[9, 8, 7])).unwrap();
        assert_eq!(&*icon.rgba, [9, 8, 7, 255]);
    }

    #[test]
    fn a_grey_icon_is_grey_in_every_channel() {
        let icon = icon_of(&png_of(1, 1, png::ColorType::Grayscale, &[50])).unwrap();
        assert_eq!(&*icon.rgba, [50, 50, 50, 255]);
    }

    #[test]
    fn what_is_no_png_is_no_icon() {
        assert!(icon_of(b"not a picture").is_err());
    }
}
