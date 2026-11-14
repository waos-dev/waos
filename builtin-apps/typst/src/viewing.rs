//! What the window shows of the document, beyond its text and pages: how large the pages are,
//! and which of the two sides a narrow window shows.

use crate::zoom::Zoom;

/// The side a window too narrow for both shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Side {
    #[default]
    Write,
    Preview,
}

#[derive(Debug)]
pub(crate) struct Viewing {
    pub(crate) zoom: Zoom,
    /// The preview's width in logical pixels and its widest page's in points, as it last laid
    /// its pages out: what the scale is told from.
    pub(crate) laid_out: (f64, f64),
    pub(crate) side: Side,
}

impl Viewing {
    /// Logical pixels to a point, at the zoom chosen in the preview as last laid out.
    pub(crate) fn scale(&self) -> f64 {
        let (preview_width, page_width) = self.laid_out;
        self.zoom.scale(preview_width, page_width)
    }
}

impl Default for Viewing {
    fn default() -> Viewing {
        Viewing {
            zoom: Zoom::default(),
            laid_out: (0.0, 0.0),
            side: Side::default(),
        }
    }
}
