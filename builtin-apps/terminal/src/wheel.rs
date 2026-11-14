//! A wheel's or a trackpad's scroll as whole lines: a trackpad scrolls a few pixels at a time,
//! and what is short of a line is kept for the next event.

/// The pixels scrolled and not yet made into a line.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Wheel {
    pending: f64,
}

impl Wheel {
    /// The whole lines `pixels` more scroll comes to, on lines `line_height` tall: positive
    /// towards the text's end, as the pixels are.
    pub(crate) fn lines(&mut self, pixels: f64, line_height: f64) -> isize {
        if line_height <= 0.0 || !pixels.is_finite() {
            return 0;
        }
        self.pending += pixels;
        let lines = (self.pending / line_height).trunc();
        self.pending -= lines * line_height;
        lines as isize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_scrolls_add_up_to_lines() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.lines(6.0, 16.0), 0);
        assert_eq!(wheel.lines(6.0, 16.0), 0);
        assert_eq!(wheel.lines(6.0, 16.0), 1);
        assert_eq!(wheel.lines(-50.0, 16.0), -3);
    }

    #[test]
    fn a_wheels_notch_is_several_lines_at_once() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.lines(48.0, 16.0), 3);
    }
}
