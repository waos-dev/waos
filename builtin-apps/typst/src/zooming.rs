//! View's commands: the pages larger, smaller, at their own size or fitted to the preview's
//! width, and, in a window too narrow for both, the text or the pages.

use waft::{App, ScrollControllerLeaf};

use crate::documents_app::DocumentsApp;
use crate::viewing::{Side, Viewing};
use crate::zoom::Zoom;

pub(crate) fn zoom_in(app: &mut App, documents: &DocumentsApp) {
    zoom(app, documents, |viewing| Zoom::stepped_in(viewing.scale()));
}

pub(crate) fn zoom_out(app: &mut App, documents: &DocumentsApp) {
    zoom(app, documents, |viewing| Zoom::stepped_out(viewing.scale()));
}

pub(crate) fn actual_size(app: &mut App, documents: &DocumentsApp) {
    zoom(app, documents, |_viewing| Zoom::Percent(100));
}

pub(crate) fn fit_width(app: &mut App, documents: &DocumentsApp) {
    zoom(app, documents, |_viewing| Zoom::FitWidth);
}

/// Fits the pages to the width, or shows them at their own size where they are fitted.
pub(crate) fn toggle_fit(app: &mut App, documents: &DocumentsApp) {
    zoom(app, documents, |viewing| match viewing.zoom {
        Zoom::FitWidth => Zoom::Percent(100),
        Zoom::Percent(_) => Zoom::FitWidth,
    });
}

/// Shows the pages at the zoom `chosen` from what is shown, scrolled to where they were: as far
/// down the pages, which are now as much higher or lower as the scale changed.
fn zoom(app: &mut App, documents: &DocumentsApp, chosen: impl FnOnce(&Viewing) -> Zoom) {
    let grown = documents.viewing.update(app, |viewing, cx| {
        let before = viewing.scale();
        viewing.zoom = chosen(viewing);
        cx.notify();
        viewing.scale() / before
    });
    let scroll = documents.preview_scroll.handle();
    if scroll.has_clients(app) && grown.is_finite() {
        let place = scroll.offset(app) * grown;
        scroll.jump_to(app, place);
    }
}

pub(crate) fn show_text(app: &mut App, documents: &DocumentsApp) {
    change(app, documents, |viewing| viewing.side = Side::Write);
}

pub(crate) fn show_pages(app: &mut App, documents: &DocumentsApp) {
    change(app, documents, |viewing| viewing.side = Side::Preview);
}

fn change(app: &mut App, documents: &DocumentsApp, change: impl FnOnce(&mut Viewing)) {
    documents.viewing.update(app, |viewing, cx| {
        change(viewing);
        cx.notify();
    });
}
