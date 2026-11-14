//! The smaller copies of an image a texture is sampled from at a distance, made on the CPU: each
//! half the size of the one before, each pixel the average of the four it covers.

/// One level of a texture: its size and its pixels, four bytes each.
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The image and every smaller copy of it, down to one pixel.
pub fn chain(width: u32, height: u32, pixels: Vec<u8>) -> Vec<Level> {
    let mut levels = vec![Level {
        width,
        height,
        pixels,
    }];
    while let Some(smaller) = levels.last().and_then(halved) {
        levels.push(smaller);
    }
    levels
}

/// `level` at half its size, rounded down but never to nothing; none for a single pixel.
fn halved(level: &Level) -> Option<Level> {
    if level.width <= 1 && level.height <= 1 {
        return None;
    }
    let (width, height) = ((level.width / 2).max(1), (level.height / 2).max(1));
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.extend(average(level, x * 2, y * 2));
        }
    }
    Some(Level {
        width,
        height,
        pixels,
    })
}

/// The average of the two-by-two block at (`x`, `y`), clamped at the image's edges.
fn average(level: &Level, x: u32, y: u32) -> [u8; 4] {
    let mut sums = [0u32; 4];
    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let column = (x + dx).min(level.width - 1);
        let row = (y + dy).min(level.height - 1);
        let start = ((row * level.width + column) * 4) as usize;
        for (sum, byte) in sums.iter_mut().zip(&level.pixels[start..start + 4]) {
            *sum += u32::from(*byte);
        }
    }
    sums.map(|sum| ((sum + 2) / 4) as u8)
}
