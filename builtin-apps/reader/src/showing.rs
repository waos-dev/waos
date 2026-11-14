//! What the user's commands show in the window: the items of every feed or of one, an item to
//! read, which is marked read as it opens, the settings, and the way back to the items.

use waft::App;

use crate::browsing::Page;
use crate::library::{ItemRef, Scope};
use crate::reader_app::ReaderApp;

/// Lists the items of `scope`.
pub(crate) fn list_items(app: &mut App, reader: &ReaderApp, scope: Scope) {
    reader.browsing.update(app, |browsing, cx| {
        browsing.list(scope);
        cx.notify();
    });
}

/// Opens `item` to read, and marks it read.
pub(crate) fn read_item(app: &mut App, reader: &ReaderApp, item: ItemRef) {
    reader.change_library(app, |library| library.mark(&item, true));
    show(app, reader, Page::Reading(item));
}

pub(crate) fn show_settings(app: &mut App, reader: &ReaderApp) {
    show(app, reader, Page::Settings);
}

/// Goes back from an item or the settings to the items listed.
pub(crate) fn back_to_items(app: &mut App, reader: &ReaderApp) {
    reader.browsing.update(app, |browsing, cx| {
        browsing.back_to_items();
        cx.notify();
    });
}

/// Marks every item listed read.
pub(crate) fn mark_listed_read(app: &mut App, reader: &ReaderApp) {
    let scope = reader.browsing.read(app).scope();
    reader.change_library(app, |library| library.mark_all_read(scope));
}

/// Marks the item being read not read, and goes back to the items.
pub(crate) fn keep_unread(app: &mut App, reader: &ReaderApp) {
    let Some(item) = reader.browsing.read(app).reading().cloned() else {
        return;
    };
    reader.change_library(app, |library| library.mark(&item, false));
    back_to_items(app, reader);
}

fn show(app: &mut App, reader: &ReaderApp, page: Page) {
    reader.browsing.update(app, |browsing, cx| {
        browsing.show(page);
        cx.notify();
    });
}
