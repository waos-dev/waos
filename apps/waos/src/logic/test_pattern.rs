//! The picture WaOS's simulated camera shows, drawn into 32-bit BGRA pixels, plain enough that a
//! test can read it back: a dark ground; a square of a colour of its own in each corner, red at the
//! top-left, green at the top-right, blue at the bottom-left and white at the bottom-right, so a
//! picture turned, flipped or with its channels swapped is told at once; a frame counter across the
//! middle, sixteen cells, white for a one and black for a zero, the highest bit at the left, so
//! two frames are told apart by their number; and a yellow bar moving left to right under it,
//! which a person sees move. Chrome's fake camera draws a sweeping arc with a timestamp and
//! coloured corner squares for its pixel tests; this keeps the corners and the counter, in cells a
//! test reads without text.
//!
//! The invariant: what [`draw`] draws, the tests' readers read back, on any size of at least 64
//! pixels each way.

/// The smallest picture the pattern is drawn on, each way, in pixels.
#[cfg(test)]
pub(crate) const SMALLEST: u32 = 64;

/// A pixel's colour, in the byte order the pixels hold: blue, green, red, alpha.
pub(crate) type Bgra = [u8; 4];

pub(crate) const RED: Bgra = [0, 0, 255, 255];
pub(crate) const GREEN: Bgra = [0, 255, 0, 255];
pub(crate) const BLUE: Bgra = [255, 0, 0, 255];
pub(crate) const WHITE: Bgra = [255, 255, 255, 255];
const BLACK: Bgra = [0, 0, 0, 255];
const GROUND: Bgra = [48, 48, 48, 255];
const YELLOW: Bgra = [0, 220, 255, 255];

/// How many cells the counter has: the frame's number, modulo 2^16.
pub(crate) const COUNTER_BITS: u32 = 16;

/// Which corner of the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    #[cfg(test)]
    pub(crate) const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    /// The colour the pattern gives the corner.
    pub(crate) fn colour(self) -> Bgra {
        match self {
            Corner::TopLeft => RED,
            Corner::TopRight => GREEN,
            Corner::BottomLeft => BLUE,
            Corner::BottomRight => WHITE,
        }
    }
}

/// Where the pattern's parts are on a picture of one size.
struct Layout {
    width: u32,
    height: u32,
    /// The side of a corner's square.
    corner: u32,
    /// The side of a counter's cell, and the counter's top.
    cell: u32,
    counter_top: u32,
    counter_left: u32,
}

impl Layout {
    fn of(width: u32, height: u32) -> Layout {
        let corner = width.min(height) / 8;
        let cell = (width / (COUNTER_BITS + 4)).max(1);
        Layout {
            width,
            height,
            corner,
            cell,
            counter_top: height / 2 - cell / 2,
            counter_left: (width - cell * COUNTER_BITS) / 2,
        }
    }

    /// The colour of the pixel at (`x`, `y`) of frame `frame`.
    fn colour_at(&self, frame: u64, x: u32, y: u32) -> Bgra {
        if let Some(corner) = self.corner_at(x, y) {
            return corner.colour();
        }
        if let Some(bit) = self.counter_bit_at(x, y) {
            let on = (frame >> (COUNTER_BITS - 1 - bit)) & 1 == 1;
            return if on { WHITE } else { BLACK };
        }
        if self.bar_at(frame, x, y) {
            return YELLOW;
        }
        GROUND
    }

    fn corner_at(&self, x: u32, y: u32) -> Option<Corner> {
        let left = x < self.corner;
        let right = x >= self.width - self.corner;
        let top = y < self.corner;
        let bottom = y >= self.height - self.corner;
        match (left, right, top, bottom) {
            (true, _, true, _) => Some(Corner::TopLeft),
            (_, true, true, _) => Some(Corner::TopRight),
            (true, _, _, true) => Some(Corner::BottomLeft),
            (_, true, _, true) => Some(Corner::BottomRight),
            _ => None,
        }
    }

    /// Which of the counter's cells (`x`, `y`) is in, the highest bit's being 0.
    fn counter_bit_at(&self, x: u32, y: u32) -> Option<u32> {
        let inside = y >= self.counter_top
            && y < self.counter_top + self.cell
            && x >= self.counter_left
            && x < self.counter_left + self.cell * COUNTER_BITS;
        inside.then(|| (x - self.counter_left) / self.cell)
    }

    /// Whether (`x`, `y`) is on the bar, a cell wide and two tall, under the counter, which moves a
    /// cell's width to the right each frame and starts again at the left.
    fn bar_at(&self, frame: u64, x: u32, y: u32) -> bool {
        let top = self.counter_top + 2 * self.cell;
        let travel = u64::from(self.width.saturating_sub(self.cell).max(1));
        let left = (frame * u64::from(self.cell)) % travel;
        y >= top
            && y < top + 2 * self.cell
            && u64::from(x) >= left
            && u64::from(x) < left + u64::from(self.cell)
    }

    /// The middle of the counter's cell `bit`.
    #[cfg(test)]
    fn cell_middle(&self, bit: u32) -> (u32, u32) {
        (
            self.counter_left + bit * self.cell + self.cell / 2,
            self.counter_top + self.cell / 2,
        )
    }
}

/// Draws frame `frame` of the pattern into `pixels`, `width` by `height` BGRA pixels, rows
/// `row_bytes` apart.
pub(crate) fn draw(frame: u64, width: u32, height: u32, row_bytes: usize, pixels: &mut [u8]) {
    let layout = Layout::of(width, height);
    for y in 0..height {
        let row = &mut pixels[y as usize * row_bytes..][..width as usize * 4];
        for (x, pixel) in (0..width).zip(row.as_chunks_mut::<4>().0.iter_mut()) {
            *pixel = layout.colour_at(frame, x, y);
        }
    }
}

/// The frame's number the counter of `pixels` shows, as [`draw`] drew it: the low 16 bits.
#[cfg(test)]
pub(crate) fn read_frame_number(width: u32, height: u32, row_bytes: usize, pixels: &[u8]) -> u16 {
    let layout = Layout::of(width, height);
    (0..COUNTER_BITS).fold(0, |number, bit| {
        let (x, y) = layout.cell_middle(bit);
        let at = y as usize * row_bytes + x as usize * 4;
        let on = pixels[at + 1] > 127;
        (number << 1) | u16::from(on)
    })
}

/// The colour in the middle of `corner`'s square of `pixels`.
#[cfg(test)]
pub(crate) fn corner_colour(
    corner: Corner,
    width: u32,
    height: u32,
    row_bytes: usize,
    pixels: &[u8],
) -> Bgra {
    let layout = Layout::of(width, height);
    let middle = layout.corner / 2;
    let x = match corner {
        Corner::TopLeft | Corner::BottomLeft => middle,
        Corner::TopRight | Corner::BottomRight => width - 1 - middle,
    };
    let y = match corner {
        Corner::TopLeft | Corner::TopRight => middle,
        Corner::BottomLeft | Corner::BottomRight => height - 1 - middle,
    };
    let at = y as usize * row_bytes + x as usize * 4;
    pixels[at..at + 4].try_into().expect("four bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(frame: u64, width: u32, height: u32) -> (usize, Vec<u8>) {
        let row_bytes = width as usize * 4 + 64;
        let mut pixels = vec![0; row_bytes * height as usize];
        draw(frame, width, height, row_bytes, &mut pixels);
        (row_bytes, pixels)
    }

    #[test]
    fn each_corner_shows_its_own_colour_at_either_format() {
        for (width, height) in [(640, 480), (1280, 720), (SMALLEST, SMALLEST)] {
            let (row_bytes, pixels) = drawn(7, width, height);
            for corner in Corner::ALL {
                assert_eq!(
                    corner_colour(corner, width, height, row_bytes, &pixels),
                    corner.colour(),
                    "{corner:?} at {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn the_counter_reads_back_each_frames_number() {
        for frame in [0, 1, 2, 255, 256, 40_000, 65_535, 65_536 + 3] {
            let (row_bytes, pixels) = drawn(frame, 640, 480);
            assert_eq!(
                read_frame_number(640, 480, row_bytes, &pixels),
                frame as u16,
                "frame {frame}"
            );
        }
    }

    #[test]
    fn two_frames_differ_and_the_bar_moves() {
        let (_, first) = drawn(10, 640, 480);
        let (_, next) = drawn(11, 640, 480);
        assert_ne!(first, next);
        let yellow = |pixels: &[u8]| {
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .position(|pixel| *pixel == YELLOW)
        };
        assert!(yellow(&first) < yellow(&next), "further right");
    }
}
