//! A feed the user follows and its items, as the library keeps them: what the feed's document said
//! at its last refresh, and what the app adds, which items were read and when each was first seen.

use serde::{Deserialize, Serialize};

use crate::http::Validators;

/// A feed's name in the library, kept from one run to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct FeedId(pub(crate) u64);

/// A feed the user follows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Feed {
    pub(crate) id: FeedId,
    /// Where its document is fetched from.
    pub(crate) address: String,
    /// What it calls itself; its address's host until its document was read once.
    pub(crate) title: String,
    /// What its server said of the document last fetched, sent back with the next request.
    #[serde(default)]
    pub(crate) validators: Validators,
    /// When its document was last fetched or found unchanged, in seconds since 1970; none until
    /// the first.
    #[serde(default)]
    pub(crate) refreshed: Option<u64>,
}

/// One item of a feed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Item {
    /// What names it within its feed: the id its feed gives it.
    pub(crate) key: String,
    pub(crate) title: String,
    /// Where the original is, to open in a browser.
    pub(crate) link: Option<String>,
    /// When its feed says it was published or last changed, in seconds since 1970.
    pub(crate) published: Option<u64>,
    /// Its text as the feed gives it, HTML.
    pub(crate) html: String,
    /// The first lines of its text, plain.
    pub(crate) preview: String,
    /// The address of the picture shown with it in a list, where it has one.
    pub(crate) picture: Option<String>,
    /// When a refresh first found it, in seconds since 1970.
    pub(crate) found: u64,
    pub(crate) read: bool,
}

impl Item {
    /// The moment it is listed by: when it was published, or else when it was found.
    pub(crate) fn listed_at(&self) -> u64 {
        self.published.unwrap_or(self.found)
    }
}

/// An item as a feed's document gives it, before the library keeps it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FetchedItem {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) link: Option<String>,
    pub(crate) published: Option<u64>,
    pub(crate) html: String,
    pub(crate) preview: String,
    pub(crate) picture: Option<String>,
}

/// What a feed's document says: its title and its items.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FetchedFeed {
    pub(crate) title: Option<String>,
    pub(crate) items: Vec<FetchedItem>,
}
