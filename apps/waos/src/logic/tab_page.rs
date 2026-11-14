//! The box a tab's page takes in the view the deck is in, which a program in a tab draws in: the
//! whole view above the deck's bar, which sits along the bottom clear of the system's own bars
//! there and of the zone where the system takes swipes for its own navigation, so a drag on the
//! bar is the deck's. The system's bars at the top and the sides still cover the page, so its
//! padding there is the view's; at the bottom the bar takes what the system covers, so the page
//! has none.
//!
//! The invariant: the page and the bar share the view between them exactly, so the canvas the
//! deck lays out in the page is the size the program is told it draws at; and a keyboard changes
//! neither, only what the page is told it covers.

use waft::ui::{ViewConstraints, ViewMetrics, ViewPadding};

/// How tall the deck's bar is above what it sits clear of, in logical points: room for the pill
/// that names the page, 42 points tall, with 7 above and below it.
pub(crate) const BAR_HEIGHT: f64 = 56.0;

/// What the bar sits clear of at the view's bottom, in the units of its two arguments: the
/// system's bar there (the bottom of the view padding), whether or not a keyboard covers it, and
/// the system's bottom gesture zone (the bottom of the system gesture insets, where Android's
/// gesture navigation takes a swipe for itself), whichever reaches higher.
pub(crate) fn beneath_the_bar(view_padding_bottom: f64, system_gesture_inset_bottom: f64) -> f64 {
    view_padding_bottom.max(system_gesture_inset_bottom)
}

/// How far up from the view's bottom the bar reaches, in physical pixels: its own height and what
/// it sits clear of beneath it.
fn bar_extent(view: &ViewMetrics) -> f64 {
    BAR_HEIGHT * view.device_pixel_ratio
        + beneath_the_bar(view.view_padding.bottom, view.system_gesture_insets.bottom)
}

/// What a tab's page measures in a view that measures `view`.
pub(crate) fn page_metrics(view: &ViewMetrics) -> ViewMetrics {
    let bar = bar_extent(view);
    let [width, height] = view.physical_size;
    let page_height = (height - bar).max(0.0);
    ViewMetrics {
        physical_size: [width, page_height],
        physical_constraints: ViewConstraints::tight(width, page_height),
        padding: without_bottom(view.padding),
        view_padding: without_bottom(view.view_padding),
        system_gesture_insets: without_bottom(view.system_gesture_insets),
        view_insets: ViewPadding {
            bottom: (view.view_insets.bottom - bar).max(0.0),
            ..view.view_insets
        },
        ..*view
    }
}

fn without_bottom(padding: ViewPadding) -> ViewPadding {
    ViewPadding {
        bottom: 0.0,
        ..padding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A phone's screen at three pixels a point, with a status bar and a home indicator.
    fn phone() -> ViewMetrics {
        let padding = ViewPadding {
            left: 0.0,
            top: 141.0,
            right: 0.0,
            bottom: 102.0,
        };
        ViewMetrics {
            physical_size: [1170.0, 2532.0],
            physical_constraints: ViewConstraints::tight(1170.0, 2532.0),
            device_pixel_ratio: 3.0,
            padding,
            view_padding: padding,
            view_insets: ViewPadding::ZERO,
            system_gesture_insets: ViewPadding::ZERO,
            screen: None,
        }
    }

    /// A phone like a Pixel 2 XL with gesture navigation, at three and a half pixels a point:
    /// Android's gesture zone along the bottom reaches higher than its navigation bar.
    fn android_phone() -> ViewMetrics {
        let padding = ViewPadding {
            left: 0.0,
            top: 84.0,
            right: 0.0,
            bottom: 84.0,
        };
        ViewMetrics {
            physical_size: [1440.0, 2880.0],
            physical_constraints: ViewConstraints::tight(1440.0, 2880.0),
            device_pixel_ratio: 3.5,
            padding,
            view_padding: padding,
            view_insets: ViewPadding::ZERO,
            system_gesture_insets: ViewPadding {
                left: 105.0,
                top: 84.0,
                right: 105.0,
                bottom: 112.0,
            },
            screen: None,
        }
    }

    #[test]
    fn the_page_is_the_view_above_the_bar_and_the_systems_bottom_padding() {
        let page = page_metrics(&phone());
        assert_eq!(page.physical_size, [1170.0, 2532.0 - 168.0 - 102.0]);
        assert_eq!(page.device_pixel_ratio, 3.0);
    }

    #[test]
    fn the_status_bar_still_covers_the_page_and_the_bottom_is_the_bars() {
        let page = page_metrics(&phone());
        assert_eq!(page.padding.top, 141.0);
        assert_eq!(page.padding.bottom, 0.0);
        assert_eq!(page.view_padding.bottom, 0.0);
    }

    #[test]
    fn a_keyboard_covers_the_page_only_above_the_bar() {
        let mut view = phone();
        view.view_insets.bottom = 1000.0;
        assert_eq!(
            page_metrics(&view).view_insets.bottom,
            1000.0 - 168.0 - 102.0
        );
        view.view_insets.bottom = 100.0;
        assert_eq!(page_metrics(&view).view_insets.bottom, 0.0);
    }

    /// A keyboard up takes the system's bar out of the view's padding, as a phone reports it.
    #[test]
    fn a_keyboard_does_not_resize_the_page() {
        let mut view = phone();
        view.view_insets.bottom = 1000.0;
        view.padding.bottom = 0.0;
        assert_eq!(
            page_metrics(&view).physical_size,
            page_metrics(&phone()).physical_size
        );
    }

    #[test]
    fn the_bar_sits_clear_of_the_systems_gesture_zone_where_it_reaches_higher() {
        let page = page_metrics(&android_phone());
        assert_eq!(page.physical_size, [1440.0, 2880.0 - 196.0 - 112.0]);
        assert_eq!(page.padding.bottom, 0.0);
        assert_eq!(page.view_padding.bottom, 0.0);
    }

    #[test]
    fn the_gesture_zone_beneath_the_bar_does_not_reach_the_page() {
        let page = page_metrics(&android_phone());
        assert_eq!(
            page.system_gesture_insets,
            ViewPadding {
                left: 105.0,
                top: 84.0,
                right: 105.0,
                bottom: 0.0,
            }
        );
    }

    #[test]
    fn a_keyboard_covers_the_page_only_above_the_bar_clear_of_the_gesture_zone() {
        let mut view = android_phone();
        view.view_insets.bottom = 1000.0;
        assert_eq!(
            page_metrics(&view).view_insets.bottom,
            1000.0 - 196.0 - 112.0
        );
    }

    #[test]
    fn a_view_shorter_than_the_bar_leaves_the_page_empty() {
        let mut view = phone();
        view.physical_size = [1170.0, 100.0];
        assert_eq!(page_metrics(&view).physical_size, [1170.0, 0.0]);
    }
}
