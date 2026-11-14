//! The list of feeds, as the items of the window's `NavigationView`: every feed's items together,
//! then each feed the user follows by its title, each with how many of its items are not read;
//! and at its foot, Add Feed. Each item is known by an id of its own, which names what it lists.

use waft::{App, Text, TextOverflow};
use waft_winui::{
    InfoBadge, NavigationView, NavigationViewItem, NavigationViewItemEventArgs,
    NavigationViewSelectionChangedEventArgs,
};

use crate::adding::ask_for_a_feed;
use crate::browsing::Page;
use crate::feed::FeedId;
use crate::icons;
use crate::library::Scope;
use crate::reader_app::ReaderApp;
use crate::showing::{list_items, show_settings};

/// The ids of the items that are no feed's.
const ALL_ITEMS: &str = "all";
const FEEDS_LINE: &str = "feeds-line";
const FEEDS: &str = "feeds";
const ADD_FEED: &str = "add-feed";

/// What the id of a feed's item begins with, before the feed's id.
const FEED_PREFIX: &str = "feed:";

/// Every feed's items, then each feed.
pub(crate) fn menu_items(app: &App, reader: &ReaderApp) -> Vec<NavigationViewItem> {
    let library = reader.library.read(app);
    let all = NavigationViewItem::text(ALL_ITEMS, "All Items").icon(icons::all_items());
    let mut items = vec![
        with_unread(all, library.unread(Scope::All)),
        NavigationViewItem::separator(FEEDS_LINE),
        NavigationViewItem::header(FEEDS, "Feeds"),
    ];
    items.extend(library.feeds().map(|feed| {
        let scope = Scope::Feed(feed.id);
        with_unread(feed_item(scope, &feed.title), library.unread(scope))
    }));
    items
}

/// The item at the list's foot: Add Feed.
pub(crate) fn footer_items() -> Vec<NavigationViewItem> {
    let add = NavigationViewItem::text(ADD_FEED, "Add Feed")
        .icon(icons::add())
        .selects_on_invoked(false);
    vec![add]
}

/// The id of the item selected while the window shows `page` of `scope`.
pub(crate) fn selected_item(scope: Scope, page: &Page) -> String {
    match page {
        Page::Settings => NavigationView::SETTINGS_ITEM_ID.to_owned(),
        Page::Items | Page::Reading(_) => item_of(scope),
    }
}

/// The user picked an item: what it lists is shown, or the settings.
pub(crate) fn picked(
    app: &mut App,
    reader: &ReaderApp,
    args: NavigationViewSelectionChangedEventArgs,
) {
    if args.is_settings {
        return show_settings(app, reader);
    }
    if let Some(scope) = args.item.and_then(|item| listed_by(&item.id)) {
        list_items(app, reader, scope);
    }
}

/// The user invoked an item: Add Feed asks for a feed, and the item selected already goes back to
/// its list from an item being read.
pub(crate) fn invoked(app: &mut App, reader: &ReaderApp, args: NavigationViewItemEventArgs) {
    if args.item.id == ADD_FEED {
        return ask_for_a_feed(app, reader);
    }
    let listed = reader.browsing.read(app).scope();
    if !args.is_settings && listed_by(&args.item.id) == Some(listed) {
        list_items(app, reader, listed);
    }
}

/// The item of the feed `scope` lists, by its `title` on one line, cut short where it is too long.
fn feed_item(scope: Scope, title: &str) -> NavigationViewItem {
    let line = Text::new(title)
        .max_lines(1)
        .overflow(TextOverflow::Ellipsis);
    let mut item = NavigationViewItem::new(item_of(scope), line).icon(icons::feed());
    // The tooltip the item shows while the list is only its icons.
    item.text = Some(title.to_owned());
    item
}

/// `item`, badged with `unread` where any of its items are not read.
fn with_unread(item: NavigationViewItem, unread: usize) -> NavigationViewItem {
    if unread == 0 {
        return item;
    }
    let count = i32::try_from(unread).unwrap_or(i32::MAX);
    item.info_badge(InfoBadge::new().value(count))
}

/// The id of the item that lists `scope`.
fn item_of(scope: Scope) -> String {
    match scope {
        Scope::All => ALL_ITEMS.to_owned(),
        Scope::Feed(id) => format!("{FEED_PREFIX}{}", id.0),
    }
}

/// What the item `item` lists; none for an item that lists nothing.
fn listed_by(item: &str) -> Option<Scope> {
    if item == ALL_ITEMS {
        return Some(Scope::All);
    }
    let id = item.strip_prefix(FEED_PREFIX)?.parse().ok()?;
    Some(Scope::Feed(FeedId(id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_names_what_it_lists() {
        for scope in [Scope::All, Scope::Feed(FeedId(7))] {
            assert_eq!(listed_by(&item_of(scope)), Some(scope));
        }
        assert_eq!(item_of(Scope::Feed(FeedId(7))), "feed:7");
    }

    #[test]
    fn an_item_that_lists_nothing_names_none() {
        for item in [ADD_FEED, FEEDS, FEEDS_LINE, "feed:", "feed:x"] {
            assert_eq!(listed_by(item), None, "{item}");
        }
    }
}
