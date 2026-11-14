//! What makes a PNG a program's icon: square, and at least [`MINIMUM_SIDE`] pixels on a side,
//! read from its header without decoding it. 1024 is recommended, since macOS icons want it.

use crate::PackageError;

/// The smallest side an icon may have, in pixels.
pub const MINIMUM_SIDE: u32 = 256;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Refuses `png` unless its header says it is a square PNG of at least [`MINIMUM_SIDE`].
pub(crate) fn check_icon(png: &[u8]) -> Result<(), PackageError> {
    let (width, height) = png_size(png).ok_or(PackageError::IconNotPng)?;
    if width != height {
        return Err(PackageError::IconNotSquare { width, height });
    }
    if width < MINIMUM_SIDE {
        return Err(PackageError::IconTooSmall { side: width });
    }
    Ok(())
}

/// A PNG's width and height, from the IHDR chunk its signature is followed by: the chunk's
/// length, its type, then the two as big-endian `u32`s at bytes 16..24.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    let header = png.get(..24)?;
    if header[..8] != PNG_SIGNATURE || &header[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::png_header;

    #[test]
    fn a_square_of_the_minimum_side_or_more_is_an_icon() {
        for side in [MINIMUM_SIDE, 1024, 4096] {
            check_icon(&png_header(side, side)).unwrap();
        }
    }

    #[test]
    fn an_icon_below_the_minimum_side_is_too_small() {
        let side = MINIMUM_SIDE - 1;
        assert!(matches!(
            check_icon(&png_header(side, side)),
            Err(PackageError::IconTooSmall { side: 255 })
        ));
    }

    #[test]
    fn an_icon_that_is_not_square_is_refused() {
        assert!(matches!(
            check_icon(&png_header(1024, 512)),
            Err(PackageError::IconNotSquare {
                width: 1024,
                height: 512
            })
        ));
    }

    #[test]
    fn bytes_that_do_not_begin_as_a_png_are_refused() {
        let jpeg = [0xff, 0xd8, 0xff, 0xe0].repeat(8);
        let truncated = png_header(1024, 1024)[..20].to_vec();
        let mut without_ihdr = png_header(1024, 1024);
        without_ihdr[12..16].copy_from_slice(b"IDAT");
        for not_png in [jpeg, truncated, without_ihdr, Vec::new()] {
            assert!(matches!(
                check_icon(&not_png),
                Err(PackageError::IconNotPng)
            ));
        }
    }
}
