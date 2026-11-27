//! What `waos package` says of a program's icon that breaks the rule every icon follows: it fills
//! its whole square, at least `waos_package::ICON_MINIMUM_SIDE` pixels on a side, with no rounded
//! corners, margin or transparency, since WaOS shapes it on each system. An icon drawn as a rounded
//! tile on a transparent margin shows smaller than every other.
//!
//! The invariant: this only warns. Whether a package may carry the icon is `waos-package`'s to
//! decide, and a PNG this cannot decode is left to it.

use png::{Decoder, Transformations};
use waos_package::ICON_MINIMUM_SIDE;

/// The rule, as a warning states it.
pub const RULE: &str = "an icon fills the whole square: no rounded corners, margin or transparency; WaOS shapes it on each system";

/// What `png` does against the rule, one line each; none for an icon that keeps it, and for bytes
/// that are not a PNG this can decode.
pub fn breaches(png: &[u8]) -> Vec<String> {
    let Some(icon) = Rgba::decode(png) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    if icon.width != icon.height {
        found.push(format!(
            "it is {}x{} pixels, not square",
            icon.width, icon.height
        ));
    }
    if icon.width.min(icon.height) < ICON_MINIMUM_SIDE {
        found.push(format!(
            "it is {}x{} pixels, under {ICON_MINIMUM_SIDE} on a side",
            icon.width, icon.height
        ));
    }
    let transparent = icon.transparent_edge_pixels();
    if transparent > 0 {
        found.push(format!(
            "{transparent} of the pixels on its edges are transparent{}",
            if icon.has_transparent_corner() {
                ", its corners among them"
            } else {
                ""
            }
        ));
    }
    found
}

/// An image's pixels, each red, green, blue and alpha, a row at a time from the top.
struct Rgba {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Rgba {
    /// `png`'s pixels, whatever its colour type and depth: a palette and a transparent colour
    /// expanded to an alpha, sixteen bits taken to eight.
    fn decode(png: &[u8]) -> Option<Rgba> {
        let mut decoder = Decoder::new(std::io::Cursor::new(png));
        decoder
            .set_transformations(Transformations::normalize_to_color8() | Transformations::ALPHA);
        let mut reader = decoder.read_info().ok()?;
        let mut pixels = vec![0; reader.output_buffer_size()?];
        let frame = reader.next_frame(&mut pixels).ok()?;
        pixels.truncate(frame.buffer_size());
        let rgba = match frame.color_type {
            png::ColorType::Rgba => pixels,
            png::ColorType::GrayscaleAlpha => pixels
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|&[gray, alpha]| [gray, gray, gray, alpha])
                .collect(),
            _ => return None,
        };
        Some(Rgba {
            width: frame.width,
            height: frame.height,
            pixels: rgba,
        })
    }

    fn alpha(&self, x: u32, y: u32) -> u8 {
        self.pixels[((y * self.width + x) * 4 + 3) as usize]
    }

    /// Each pixel of the outermost rows and columns, once.
    fn edge(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let (right, bottom) = (self.width - 1, self.height - 1);
        (0..self.height)
            .flat_map(move |y| (0..self.width).map(move |x| (x, y)))
            .filter(move |&(x, y)| x == 0 || y == 0 || x == right || y == bottom)
    }

    /// How many pixels on the edges are not wholly opaque.
    fn transparent_edge_pixels(&self) -> usize {
        self.edge()
            .filter(|&(x, y)| self.alpha(x, y) < u8::MAX)
            .count()
    }

    fn has_transparent_corner(&self) -> bool {
        let (right, bottom) = (self.width - 1, self.height - 1);
        [(0, 0), (right, 0), (0, bottom), (right, bottom)]
            .into_iter()
            .any(|(x, y)| self.alpha(x, y) < u8::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG `width` by `height`, opaque but where `transparent` says, as RGBA.
    fn png(width: u32, height: u32, transparent: impl Fn(u32, u32) -> bool) -> Vec<u8> {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let alpha = if transparent(x, y) { 0 } else { u8::MAX };
                pixels.extend_from_slice(&[10, 132, 255, alpha]);
            }
        }
        let mut encoded = Vec::new();
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();
        encoded
    }

    #[test]
    fn an_opaque_square_of_the_minimum_side_keeps_the_rule() {
        assert!(breaches(&png(256, 256, |_, _| false)).is_empty());
    }

    #[test]
    fn a_rounded_tile_on_a_transparent_margin_is_said_with_its_corners() {
        let margin = |x: u32, y: u32| !(16..240).contains(&x) || !(16..240).contains(&y);
        let found = breaches(&png(256, 256, margin));
        assert_eq!(
            found,
            ["1020 of the pixels on its edges are transparent, its corners among them"]
        );
    }

    #[test]
    fn rounded_corners_alone_are_said() {
        let corner = |x: u32, y: u32| !(4..252).contains(&x) && !(4..252).contains(&y);
        let found = breaches(&png(256, 256, corner));
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("its corners among them"), "{found:?}");
    }

    #[test]
    fn an_icon_neither_square_nor_large_enough_is_said_both_ways() {
        let found = breaches(&png(128, 64, |_, _| false));
        assert_eq!(
            found,
            [
                "it is 128x64 pixels, not square",
                "it is 128x64 pixels, under 256 on a side"
            ]
        );
    }

    #[test]
    fn bytes_that_are_not_a_png_are_left_to_the_package() {
        assert!(breaches(b"not a png").is_empty());
    }
}
