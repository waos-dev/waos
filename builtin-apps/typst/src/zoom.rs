//! How large the pages are shown: fitted to the preview's width, or at a percentage of their
//! size, where 100% shows a typographic point as a logical pixel.

/// The percentages zooming in and out steps through.
const STEPS: [u32; 11] = [25, 50, 75, 100, 125, 150, 200, 300, 400, 600, 800];

/// The room left on each side of the widest page fitted to the preview, in logical pixels.
pub(crate) const PAGE_MARGIN: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Zoom {
    /// The widest page fills the preview's width.
    #[default]
    FitWidth,
    Percent(u32),
}

impl Zoom {
    /// Logical pixels to a point, for pages at most `page_width` points wide in a preview
    /// `preview_width` logical pixels wide.
    pub(crate) fn scale(self, preview_width: f64, page_width: f64) -> f64 {
        match self {
            Zoom::FitWidth => fitting_scale(preview_width, page_width),
            Zoom::Percent(percent) => f64::from(percent) / 100.0,
        }
    }

    /// The next step larger than `scale`.
    pub(crate) fn stepped_in(scale: f64) -> Zoom {
        let percent = percent_of(scale);
        let larger = STEPS.iter().copied().find(|step| *step > percent);
        Zoom::Percent(larger.unwrap_or(STEPS[STEPS.len() - 1]))
    }

    /// The next step smaller than `scale`.
    pub(crate) fn stepped_out(scale: f64) -> Zoom {
        let percent = percent_of(scale);
        let smaller = STEPS.iter().rev().copied().find(|step| *step < percent);
        Zoom::Percent(smaller.unwrap_or(STEPS[0]))
    }
}

/// `scale` as a whole percentage.
pub(crate) fn percent_of(scale: f64) -> u32 {
    (scale * 100.0).round().max(1.0) as u32
}

fn fitting_scale(preview_width: f64, page_width: f64) -> f64 {
    let room = (preview_width - 2.0 * PAGE_MARGIN).max(1.0);
    if page_width <= 0.0 {
        return 1.0;
    }
    room / page_width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fitted_page_leaves_its_margins() {
        let scale = Zoom::FitWidth.scale(632.0, 600.0);
        assert!((scale - 1.0).abs() < 1e-9);
    }

    #[test]
    fn zooming_steps_from_the_scale_shown() {
        assert_eq!(Zoom::stepped_in(0.83), Zoom::Percent(100));
        assert_eq!(Zoom::stepped_in(1.0), Zoom::Percent(125));
        assert_eq!(Zoom::stepped_out(1.0), Zoom::Percent(75));
        assert_eq!(Zoom::stepped_out(0.1), Zoom::Percent(25));
        assert_eq!(Zoom::stepped_in(9.0), Zoom::Percent(800));
    }
}
