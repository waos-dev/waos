//! The icon a new program starts with: a white disc on a blue square that fills it to its edges,
//! as a new Waft app's does and every icon must (`icon_rule`), 1024 pixels on a side, the size
//! `waos-package` recommends, since macOS icons want it.

/// The icon's file, beside the program's `Cargo.toml`.
pub const FILE: &str = "icon.png";

/// The icon's side, in pixels.
const SIDE: u32 = 1024;

/// The disc's radius, in pixels.
const RADIUS: f32 = 300.0;

/// The tile's colour, as red, green and blue.
const BLUE: [u8; 3] = [10, 132, 255];

/// The icon, as a PNG.
pub fn placeholder() -> Vec<u8> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, SIDE, SIDE);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .expect("a PNG's header is written into memory");
    writer
        .write_image_data(&pixels())
        .expect("the pixels are the header's size");
    writer.finish().expect("a PNG is finished in memory");
    png
}

/// The icon's pixels, a row at a time from the top, each red, green and blue: the tile's colour,
/// mixed towards white by how much of the pixel the disc covers, so its edge is smooth.
fn pixels() -> Vec<u8> {
    let center = SIDE as f32 / 2.0;
    let mut pixels = Vec::with_capacity((SIDE * SIDE * 3) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let coverage = (RADIUS + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            pixels.extend(BLUE.map(|tile| (tile as f32 + (255.0 - tile as f32) * coverage) as u8));
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_keeps_the_rule_of_icons() {
        assert!(crate::icon_rule::breaches(&placeholder()).is_empty());
    }

    #[test]
    fn the_icon_is_a_png_of_its_side_blue_at_the_corner_and_white_at_the_center() {
        let png = placeholder();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((frame.width, frame.height), (SIDE, SIDE));
        let pixel = |x: u32, y: u32| {
            let at = ((y * SIDE + x) * 3) as usize;
            [pixels[at], pixels[at + 1], pixels[at + 2]]
        };
        assert_eq!(pixel(0, 0), BLUE);
        assert_eq!(pixel(SIDE / 2, SIDE / 2), [255, 255, 255]);
    }
}
