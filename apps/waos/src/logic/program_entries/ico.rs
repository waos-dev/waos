//! A program's icon as Windows takes one from a file: an `.ico`, the package's PNG scaled to each
//! size the shell asks for, as Waft's CLI writes an executable's icon, so the taskbar, a pin and
//! the Start menu each find theirs. Windows drops a window's whole relaunch, command and name too,
//! when its icon cannot be loaded (measured on Windows 11), so the file is
//! one every reader of icons takes.
//!
//! The invariants a later editor will break: the directory's entries are in the order of the
//! images after it, each with its image's offset from the file's start and its length; and only
//! the largest image is a PNG, as Windows' own icons store it, the others bitmaps, which every
//! reader takes.

use image::RgbaImage;
use image::imageops::FilterType;

/// The sizes Windows asks an icon for: the title bar's and the taskbar's at each scale, Explorer's
/// views, and 256 for the largest.
const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

/// The largest size, stored as a PNG.
const PNG_SIZE: u32 = 256;

/// The length of the file's header and of each entry of its directory.
const HEADER: usize = 6;
const ENTRY: usize = 16;

/// The `.ico` of the picture `png` is, scaled to each of [`SIZES`]; why not, where it is no
/// picture.
pub(crate) fn ico_of(png: &[u8]) -> Result<Vec<u8>, image::ImageError> {
    let picture = image::load_from_memory_with_format(png, image::ImageFormat::Png)?.to_rgba8();
    let images: Vec<(u32, Vec<u8>)> = SIZES
        .iter()
        .map(|&size| (size, image_of(&picture, size)))
        .collect();
    Ok(file_of(&images))
}

/// The file holding `images`, each its size and its bytes: the header, the directory, then the
/// images in order.
fn file_of(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let count = u16::try_from(images.len()).expect("a few sizes");
    let mut file = Vec::new();
    for value in [0, 1, count] {
        file.extend_from_slice(&value.to_le_bytes());
    }
    let mut offset = HEADER + ENTRY * images.len();
    for (size, bytes) in images {
        // 256 is written as 0, which a byte cannot hold.
        let side = u8::try_from(*size).unwrap_or(0);
        file.extend_from_slice(&[side, side, 0, 0]);
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        let length = u32::try_from(bytes.len()).expect("an icon's image is small");
        file.extend_from_slice(&length.to_le_bytes());
        let at = u32::try_from(offset).expect("an icon is small");
        file.extend_from_slice(&at.to_le_bytes());
        offset += bytes.len();
    }
    for (_, bytes) in images {
        file.extend_from_slice(bytes);
    }
    file
}

/// One image of the icon, `size` pixels square: a PNG at [`PNG_SIZE`], a bitmap below it.
fn image_of(picture: &RgbaImage, size: u32) -> Vec<u8> {
    let scaled = image::imageops::resize(picture, size, size, FilterType::Lanczos3);
    if size == PNG_SIZE {
        png(&scaled)
    } else {
        bitmap(&scaled)
    }
}

fn png(image: &RgbaImage) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encoding a PNG in memory");
    bytes.into_inner()
}

/// An icon's bitmap: its header, which counts the colour rows and the mask's together as the
/// height, then its rows of BGRA from the bottom, then the mask, all clear, since the alpha says
/// what shows.
fn bitmap(image: &RgbaImage) -> Vec<u8> {
    let (width, height) = image.dimensions();
    let mask_row = width.div_ceil(32) * 4;
    let mut bytes = Vec::new();
    for value in [40, width, height * 2] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    let image_bytes = width * height * 4 + mask_row * height;
    for value in [0, image_bytes, 0, 0, 0, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for row in (0..height).rev() {
        for column in 0..width {
            let [red, green, blue, alpha] = image.get_pixel(column, row).0;
            bytes.extend_from_slice(&[blue, green, red, alpha]);
        }
    }
    bytes.resize(bytes.len() + (mask_row * height) as usize, 0);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le16(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn le32(bytes: &[u8], at: usize) -> usize {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
    }

    fn picture_png(side: u32) -> Vec<u8> {
        let picture = RgbaImage::from_pixel(side, side, image::Rgba([0x3b, 0x82, 0xf6, 0xff]));
        png(&picture)
    }

    #[test]
    fn the_icon_holds_every_size_each_where_its_entry_says() {
        let ico = ico_of(&picture_png(1024)).unwrap();
        assert_eq!((le16(&ico, 0), le16(&ico, 2), le16(&ico, 4)), (0, 1, 8));
        let mut expected_offset = HEADER + ENTRY * SIZES.len();
        for (index, size) in SIZES.iter().enumerate() {
            let entry = HEADER + ENTRY * index;
            let side = if *size == 256 { 0 } else { *size as u8 };
            assert_eq!(&ico[entry..entry + 4], &[side, side, 0, 0], "{size}");
            assert_eq!((le16(&ico, entry + 4), le16(&ico, entry + 6)), (1, 32));
            let (length, offset) = (le32(&ico, entry + 8), le32(&ico, entry + 12));
            assert_eq!(offset, expected_offset, "{size}");
            let image = &ico[offset..offset + length];
            if *size == PNG_SIZE {
                let decoded = image::load_from_memory(image).unwrap();
                assert_eq!((decoded.width(), decoded.height()), (256, 256));
            } else {
                assert_eq!(le32(image, 0), 40, "a bitmap's header");
                assert_eq!(
                    (le32(image, 4), le32(image, 8)),
                    (*size as usize, *size as usize * 2)
                );
            }
            expected_offset += length;
        }
        assert_eq!(expected_offset, ico.len(), "nothing after the last image");
    }

    #[test]
    fn a_bitmap_keeps_the_pictures_colour_and_alpha_bottom_up_in_bgra() {
        let ico = ico_of(&picture_png(64)).unwrap();
        let (length, offset) = (le32(&ico, HEADER + 8), le32(&ico, HEADER + 12));
        let sixteen = &ico[offset..offset + length];
        assert_eq!(&sixteen[40..44], &[0xf6, 0x82, 0x3b, 0xff]);
    }

    #[test]
    fn what_is_no_png_makes_no_icon() {
        assert!(ico_of(b"not a picture").is_err());
    }
}
