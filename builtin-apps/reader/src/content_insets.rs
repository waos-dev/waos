//! How far the content keeps from its room's edges: at its left, in line with the header's title
//! where the list of feeds is a sidebar, as WinUI's gallery lays out its pages, and a phone's
//! margin where the feeds are a pane over it, so a narrow window spends none of its width on air.

use waft::{App, BuildContext};
use waft_winui::ThemeResources;

use crate::reader_app::ReaderApp;

/// The margin at each side at a phone's width, in logical points: the room a row's dot takes, so
/// that a row's words and an item's text begin on one line.
const NARROW_MARGIN: f64 = 20.0;

/// The margin at the right beside a sidebar, in logical points.
const WIDE_RIGHT_MARGIN: f64 = 24.0;

/// The space the content leaves at its left and at its right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Insets {
    pub(crate) left: f64,
    pub(crate) right: f64,
}

pub(crate) fn content_insets(app: &mut App, context: BuildContext, reader: &ReaderApp) -> Insets {
    if reader.browsing.read(app).narrow() {
        return Insets {
            left: NARROW_MARGIN,
            right: NARROW_MARGIN,
        };
    }
    let resources = ThemeResources::of(app, context);
    Insets {
        left: resources.navigation_view_header_margin[0],
        right: WIDE_RIGHT_MARGIN,
    }
}
