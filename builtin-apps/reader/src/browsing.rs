//! What the window shows: the items of every feed or of one, and over them an item being read or
//! the settings; and how the window lays out the list of feeds, beside the items or as a pane
//! over them at a phone's width.

use waft_winui::InfoBarSeverity;

use crate::feed::FeedId;
use crate::library::{ItemRef, Scope};

/// What the window's content is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    /// The items listed.
    Items,
    /// An item being read, over the list it was opened from.
    Reading(ItemRef),
    Settings,
}

/// Something the window says above its content until the user closes it or the next is said.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Notice {
    pub(crate) severity: InfoBarSeverity,
    pub(crate) message: String,
}

#[derive(Debug)]
pub(crate) struct Browsing {
    scope: Scope,
    page: Page,
    /// Whether the list of feeds is open, where it is a pane over the content.
    pane_open: bool,
    /// Whether the window is so narrow that the list of feeds is a pane over the content.
    narrow: bool,
    notice: Option<Notice>,
}

impl Default for Browsing {
    /// As the window's navigation starts, before its first layout: narrow, its pane open. The
    /// navigation tells of its layout only as it changes from that.
    fn default() -> Browsing {
        Browsing {
            scope: Scope::All,
            page: Page::Items,
            pane_open: true,
            narrow: true,
            notice: None,
        }
    }
}

impl Browsing {
    pub(crate) fn scope(&self) -> Scope {
        self.scope
    }

    pub(crate) fn page(&self) -> &Page {
        &self.page
    }

    /// The item being read.
    pub(crate) fn reading(&self) -> Option<&ItemRef> {
        match &self.page {
            Page::Reading(item) => Some(item),
            _ => None,
        }
    }

    /// Lists the items of `scope`, closing the pane where it covers them.
    pub(crate) fn list(&mut self, scope: Scope) {
        self.scope = scope;
        self.show(Page::Items);
    }

    /// Shows `page`, closing the pane where it covers it.
    pub(crate) fn show(&mut self, page: Page) {
        self.page = page;
        if self.narrow {
            self.pane_open = false;
        }
    }

    /// Goes back from an item or the settings to the items listed.
    pub(crate) fn back_to_items(&mut self) {
        self.page = Page::Items;
    }

    /// The feed `id` is followed no more: where its items were listed, every feed's are.
    pub(crate) fn feed_gone(&mut self, id: FeedId) {
        if self.scope == Scope::Feed(id) {
            self.scope = Scope::All;
        }
        if self.reading().is_some_and(|item| item.feed == id) {
            self.page = Page::Items;
        }
    }

    pub(crate) fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub(crate) fn say(&mut self, notice: Notice) {
        self.notice = Some(notice);
    }

    pub(crate) fn clear_notice(&mut self) {
        self.notice = None;
    }

    pub(crate) fn pane_open(&self) -> bool {
        self.pane_open
    }

    pub(crate) fn set_pane_open(&mut self, open: bool) {
        self.pane_open = open;
    }

    pub(crate) fn narrow(&self) -> bool {
        self.narrow
    }

    pub(crate) fn set_narrow(&mut self, narrow: bool) {
        self.narrow = narrow;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(feed: u64) -> ItemRef {
        ItemRef {
            feed: FeedId(feed),
            key: "a".to_owned(),
        }
    }

    #[test]
    fn a_pane_over_the_content_closes_as_something_is_shown() {
        let mut browsing = Browsing::default();
        browsing.set_narrow(false);
        browsing.list(Scope::Feed(FeedId(1)));
        assert!(browsing.pane_open(), "a sidebar stays");

        browsing.set_narrow(true);
        browsing.list(Scope::Feed(FeedId(2)));
        assert!(!browsing.pane_open());
        assert_eq!(browsing.scope(), Scope::Feed(FeedId(2)));
    }

    #[test]
    fn going_back_from_an_item_lists_what_was_listed() {
        let mut browsing = Browsing::default();
        browsing.list(Scope::Feed(FeedId(2)));
        browsing.show(Page::Reading(item(2)));
        assert_eq!(browsing.reading(), Some(&item(2)));
        browsing.back_to_items();
        assert_eq!(browsing.page(), &Page::Items);
        assert_eq!(browsing.scope(), Scope::Feed(FeedId(2)));
    }

    #[test]
    fn a_feed_unfollowed_leaves_neither_its_list_nor_its_item_shown() {
        let mut browsing = Browsing::default();
        browsing.list(Scope::Feed(FeedId(2)));
        browsing.show(Page::Reading(item(2)));
        browsing.feed_gone(FeedId(2));
        assert_eq!(browsing.scope(), Scope::All);
        assert_eq!(browsing.page(), &Page::Items);
    }
}
