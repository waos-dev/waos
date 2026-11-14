//! Sizes and rectangles between Waft's terms and a program's: a view's metrics, in physical
//! pixels on both sides; a window's frame and a screen's, in logical points from the top-left of
//! the main screen, y down, on both sides; and a rectangle a program drew, in its view's physical pixels, as the view's
//! logical ones, which Waft's text input places the input method's windows by.
//!
//! The invariant: a side a program draws at is never zero, since a program cannot draw into an
//! empty texture.

use waft::ui::{ViewMetrics, ViewPadding};
use waos::bindings::wasdk::platform::views::{Insets, Metrics, PixelRect};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};

/// A view's metrics as Waft reports them, as the program reads them.
pub fn metrics_of(metrics: &ViewMetrics) -> Metrics {
    Metrics {
        width: drawable_side(metrics.physical_size[0]),
        height: drawable_side(metrics.physical_size[1]),
        device_pixel_ratio: metrics.device_pixel_ratio,
        padding: insets_of(metrics.padding),
        view_padding: insets_of(metrics.view_padding),
        view_insets: insets_of(metrics.view_insets),
    }
}

/// A side of `pixels` physical pixels, as whole pixels, one at least.
fn drawable_side(pixels: f64) -> u32 {
    pixels.round().max(1.0) as u32
}

fn insets_of(padding: ViewPadding) -> Insets {
    Insets {
        left: padding.left,
        top: padding.top,
        right: padding.right,
        bottom: padding.bottom,
    }
}

/// A rectangle in a view's physical pixels, as the view's logical ones, `ratio` of the one to
/// the other.
pub(crate) fn logical_rect(rect: PixelRect, ratio: f64) -> waft::Rect {
    waft::Rect::from_ltwh(
        rect.x / ratio,
        rect.y / ratio,
        rect.width / ratio,
        rect.height / ratio,
    )
}

/// A window's frame or a screen's as Waft gives it, as the program reads it.
pub fn rect_of(rect: waft::Rect) -> Rect {
    Rect {
        origin: Point {
            x: rect.left,
            y: rect.top,
        },
        size: Size {
            width: rect.width(),
            height: rect.height(),
        },
    }
}

/// A window's size as the program gives it, as Waft takes it: both in logical points.
pub(crate) fn waft_size(size: Size) -> waft::Size {
    waft::Size::new(size.width, size.height)
}

/// A window's frame as the program gives it, as Waft takes it.
pub(crate) fn waft_rect(rect: Rect) -> waft::Rect {
    waft::Rect::from_ltwh(
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    )
}

#[cfg(test)]
mod tests {
    use waft::ui::ScreenId;

    use super::*;

    #[test]
    fn a_views_metrics_are_its_physical_pixels() {
        let bar = ViewPadding {
            left: 0.0,
            top: 56.0,
            right: 0.0,
            bottom: 0.0,
        };
        let metrics = metrics_of(&ViewMetrics {
            physical_size: [1600.0, 1000.4],
            device_pixel_ratio: 2.0,
            padding: bar,
            view_padding: bar,
            screen: Some(ScreenId(1)),
            ..ViewMetrics::default()
        });
        assert_eq!((metrics.width, metrics.height), (1600, 1000));
        assert_eq!(metrics.device_pixel_ratio, 2.0);
        assert_eq!(metrics.padding.top, 56.0);
        assert_eq!(metrics.view_padding.top, 56.0);
        assert_eq!(metrics.view_insets.bottom, 0.0);
    }

    #[test]
    fn a_view_of_no_size_is_still_one_pixel() {
        let metrics = metrics_of(&ViewMetrics::default());
        assert_eq!((metrics.width, metrics.height), (1, 1));
    }

    #[test]
    fn a_rect_the_program_drew_is_in_the_views_logical_pixels() {
        let caret = PixelRect {
            x: 20.0,
            y: 40.0,
            width: 4.0,
            height: 30.0,
        };
        assert_eq!(
            logical_rect(caret, 2.0),
            waft::Rect::from_ltwh(10.0, 20.0, 2.0, 15.0)
        );
    }

    #[test]
    fn a_frame_survives_the_trip_through_waft() {
        let frame = Rect {
            origin: Point { x: 12.0, y: 34.0 },
            size: Size {
                width: 320.0,
                height: 200.0,
            },
        };
        let back = rect_of(waft_rect(frame));
        assert_eq!((back.origin.x, back.origin.y), (12.0, 34.0));
        assert_eq!((back.size.width, back.size.height), (320.0, 200.0));
    }
}
