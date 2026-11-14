//! The feeds the user follows and the items kept of each, newest first: what the lists show, what
//! a refresh adds to, and where the read marks are. Plain data: it fetches nothing and writes
//! nothing itself, and says which feeds changed so that the store keeps only those.
//!
//! The invariant a later editor will break: an item is known by its feed and its key, and a
//! refresh that finds a key again changes what the feed says of the item, never when it was found
//! or whether it was read.

use std::collections::BTreeSet;

use crate::feed::{Feed, FeedId, FetchedFeed, FetchedItem, Item};
use crate::http::Validators;

/// The most items kept of one feed, the newest; a feed's document rarely lists more.
const KEPT_ITEMS: usize = 200;

/// A feed with the items kept of it, newest first.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Followed {
    pub(crate) feed: Feed,
    pub(crate) items: Vec<Item>,
}

/// Which items a list shows: every feed's, or one feed's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    All,
    Feed(FeedId),
}

/// What names an item across the library.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ItemRef {
    pub(crate) feed: FeedId,
    pub(crate) key: String,
}

/// An item as a list shows it, with the feed it is of.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Listed<'a> {
    pub(crate) feed: &'a Feed,
    pub(crate) item: &'a Item,
}

impl Listed<'_> {
    pub(crate) fn item_ref(&self) -> ItemRef {
        ItemRef {
            feed: self.feed.id,
            key: self.item.key.clone(),
        }
    }
}

/// What the store has yet to keep.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Changes {
    /// Whether the list of feeds changed: one added or removed, or what is kept of a feed itself.
    pub(crate) feeds: bool,
    /// The feeds whose items changed.
    pub(crate) items: BTreeSet<FeedId>,
    /// The feeds no longer followed, whose items are kept no more.
    pub(crate) removed: BTreeSet<FeedId>,
}

#[derive(Debug, Default)]
pub(crate) struct Library {
    followed: Vec<Followed>,
    changes: Changes,
}

impl Library {
    /// The library of `followed`, as the store kept it.
    pub(crate) fn of(followed: Vec<Followed>) -> Library {
        Library {
            followed,
            changes: Changes::default(),
        }
    }

    pub(crate) fn feeds(&self) -> impl Iterator<Item = &Feed> {
        self.followed.iter().map(|followed| &followed.feed)
    }

    pub(crate) fn followed(&self, id: FeedId) -> Option<&Followed> {
        self.followed.iter().find(|followed| followed.feed.id == id)
    }

    pub(crate) fn feed(&self, id: FeedId) -> Option<&Feed> {
        self.followed(id).map(|followed| &followed.feed)
    }

    /// The feed fetched from `address`, where the user follows it already.
    pub(crate) fn feed_at(&self, address: &str) -> Option<&Feed> {
        self.feeds().find(|feed| feed.address == address)
    }

    /// Follows the feed at `address`, called `title` until its document says what it is called.
    pub(crate) fn follow(&mut self, address: String, title: String) -> FeedId {
        let id = FeedId(self.feeds().map(|feed| feed.id.0 + 1).max().unwrap_or(1));
        let feed = Feed {
            id,
            address,
            title,
            validators: Validators::default(),
            refreshed: None,
        };
        self.followed.push(Followed {
            feed,
            items: Vec::new(),
        });
        self.changes.feeds = true;
        self.changes.removed.remove(&id);
        id
    }

    /// Follows the feed `id` no more, and forgets its items.
    pub(crate) fn unfollow(&mut self, id: FeedId) {
        self.followed.retain(|followed| followed.feed.id != id);
        self.changes.feeds = true;
        self.changes.items.remove(&id);
        self.changes.removed.insert(id);
    }

    /// Takes in what the document of the feed `id` said at `now`: its title, each item it had not
    /// listed before, and what it now says of those it had. Answers how many it had not listed.
    pub(crate) fn merge(&mut self, id: FeedId, fetched: FetchedFeed, now: u64) -> usize {
        let Some(followed) = self.followed_mut(id) else {
            return 0;
        };
        if let Some(title) = fetched.title.filter(|title| !title.is_empty()) {
            followed.feed.title = title;
        }
        let mut found = 0;
        for item in fetched.items {
            match followed.items.iter_mut().find(|kept| kept.key == item.key) {
                Some(kept) => say_again(kept, item),
                None => {
                    followed.items.push(found_at(item, now));
                    found += 1;
                }
            }
        }
        keep_the_newest(&mut followed.items);
        self.changes.feeds = true;
        self.changes.items.insert(id);
        found
    }

    /// Notes that the feed `id` was fetched, or found unchanged, at `now`, with what its server
    /// said of the document, and from `address` where the server said it has moved for good.
    pub(crate) fn refreshed(
        &mut self,
        id: FeedId,
        validators: Validators,
        moved_to: Option<String>,
        now: u64,
    ) {
        let Some(followed) = self.followed_mut(id) else {
            return;
        };
        followed.feed.validators = validators;
        followed.feed.refreshed = Some(now);
        if let Some(address) = moved_to {
            followed.feed.address = address;
        }
        self.changes.feeds = true;
    }

    /// The items `scope` lists, newest first.
    pub(crate) fn listed(&self, scope: Scope) -> Vec<Listed<'_>> {
        let mut listed: Vec<Listed<'_>> = self
            .in_scope(scope)
            .flat_map(|followed| {
                let feed = &followed.feed;
                followed.items.iter().map(move |item| Listed { feed, item })
            })
            .collect();
        listed.sort_by_key(|listed| std::cmp::Reverse(listed.item.listed_at()));
        listed
    }

    pub(crate) fn item(&self, item: &ItemRef) -> Option<Listed<'_>> {
        let followed = self.followed(item.feed)?;
        let found = followed.items.iter().find(|kept| kept.key == item.key)?;
        Some(Listed {
            feed: &followed.feed,
            item: found,
        })
    }

    /// How many items of `scope` are not read.
    pub(crate) fn unread(&self, scope: Scope) -> usize {
        self.in_scope(scope)
            .flat_map(|followed| &followed.items)
            .filter(|item| !item.read)
            .count()
    }

    /// Marks `item` read, or not read.
    pub(crate) fn mark(&mut self, item: &ItemRef, read: bool) {
        let kept = self
            .followed_mut(item.feed)
            .and_then(|followed| followed.items.iter_mut().find(|kept| kept.key == item.key));
        if let Some(kept) = kept.filter(|kept| kept.read != read) {
            kept.read = read;
            self.changes.items.insert(item.feed);
        }
    }

    /// Marks every item of `scope` read.
    pub(crate) fn mark_all_read(&mut self, scope: Scope) {
        for followed in &mut self.followed {
            let in_scope = scope == Scope::All || scope == Scope::Feed(followed.feed.id);
            if in_scope && followed.items.iter().any(|item| !item.read) {
                followed.items.iter_mut().for_each(|item| item.read = true);
                self.changes.items.insert(followed.feed.id);
            }
        }
    }

    /// What changed since this was last asked, for the store to keep.
    pub(crate) fn take_changes(&mut self) -> Changes {
        std::mem::take(&mut self.changes)
    }

    /// Notes `changes` as yet to keep again, where the store could not keep them.
    pub(crate) fn still_to_keep(&mut self, changes: Changes) {
        self.changes.feeds |= changes.feeds;
        self.changes.items.extend(changes.items);
        self.changes.removed.extend(changes.removed);
    }

    fn followed_mut(&mut self, id: FeedId) -> Option<&mut Followed> {
        self.followed
            .iter_mut()
            .find(|followed| followed.feed.id == id)
    }

    fn in_scope(&self, scope: Scope) -> impl Iterator<Item = &Followed> {
        self.followed
            .iter()
            .filter(move |followed| scope == Scope::All || scope == Scope::Feed(followed.feed.id))
    }
}

/// `kept`, saying what its feed now says of it.
fn say_again(kept: &mut Item, said: FetchedItem) {
    kept.title = said.title;
    kept.link = said.link;
    kept.published = said.published.or(kept.published);
    kept.html = said.html;
    kept.preview = said.preview;
    kept.picture = said.picture;
}

/// `item`, kept as first found at `now`, not read.
fn found_at(item: FetchedItem, now: u64) -> Item {
    Item {
        key: item.key,
        title: item.title,
        link: item.link,
        published: item.published,
        html: item.html,
        preview: item.preview,
        picture: item.picture,
        found: now,
        read: false,
    }
}

/// Sorts `items` newest first, and lets go of those past [`KEPT_ITEMS`].
fn keep_the_newest(items: &mut Vec<Item>) {
    items.sort_by_key(|item| std::cmp::Reverse(item.listed_at()));
    items.truncate(KEPT_ITEMS);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetched(key: &str, published: u64) -> FetchedItem {
        FetchedItem {
            key: key.to_owned(),
            title: format!("Title of {key}"),
            link: Some(format!("https://example.org/{key}")),
            published: Some(published),
            html: "<p>Text</p>".to_owned(),
            preview: "Text".to_owned(),
            picture: None,
        }
    }

    fn document(items: Vec<FetchedItem>) -> FetchedFeed {
        FetchedFeed {
            title: Some("Example".to_owned()),
            items,
        }
    }

    fn library_following_one() -> (Library, FeedId) {
        let mut library = Library::default();
        let id = library.follow(
            "https://example.org/feed".to_owned(),
            "example.org".to_owned(),
        );
        (library, id)
    }

    fn keys(library: &Library, scope: Scope) -> Vec<&str> {
        let listed = library.listed(scope);
        listed
            .iter()
            .map(|listed| listed.item.key.as_str())
            .collect()
    }

    #[test]
    fn a_refresh_adds_what_is_new_and_names_the_feed() {
        let (mut library, id) = library_following_one();
        let found = library.merge(id, document(vec![fetched("a", 10), fetched("b", 20)]), 100);
        assert_eq!(found, 2);
        assert_eq!(library.feed(id).unwrap().title, "Example");
        assert_eq!(keys(&library, Scope::All), ["b", "a"], "newest first");

        let found = library.merge(id, document(vec![fetched("b", 20), fetched("c", 30)]), 200);
        assert_eq!(found, 1);
        assert_eq!(keys(&library, Scope::Feed(id)), ["c", "b", "a"]);
    }

    #[test]
    fn an_item_found_again_keeps_its_read_mark_and_takes_its_new_text() {
        let (mut library, id) = library_following_one();
        library.merge(id, document(vec![fetched("a", 10)]), 100);
        let a = ItemRef {
            feed: id,
            key: "a".to_owned(),
        };
        library.mark(&a, true);

        let mut corrected = fetched("a", 10);
        corrected.title = "Corrected".to_owned();
        library.merge(id, document(vec![corrected]), 200);

        let kept = library.item(&a).unwrap().item;
        assert!(kept.read);
        assert_eq!(kept.found, 100);
        assert_eq!(kept.title, "Corrected");
    }

    #[test]
    fn unread_counts_follow_the_marks() {
        let (mut library, first) = library_following_one();
        let second = library.follow("https://example.com/feed".to_owned(), "b".to_owned());
        library.merge(
            first,
            document(vec![fetched("a", 10), fetched("b", 20)]),
            100,
        );
        library.merge(second, document(vec![fetched("c", 30)]), 100);
        assert_eq!(library.unread(Scope::All), 3);

        library.mark_all_read(Scope::Feed(first));
        assert_eq!(library.unread(Scope::All), 1);
        assert_eq!(library.unread(Scope::Feed(first)), 0);
        assert_eq!(library.unread(Scope::Feed(second)), 1);
    }

    #[test]
    fn only_the_newest_items_of_a_feed_are_kept() {
        let (mut library, id) = library_following_one();
        let many = (0..KEPT_ITEMS as u64 + 5)
            .map(|n| fetched(&format!("item-{n}"), n))
            .collect();
        library.merge(id, document(many), 100);
        let listed = library.listed(Scope::All);
        assert_eq!(listed.len(), KEPT_ITEMS);
        assert_eq!(listed.last().unwrap().item.key, "item-5");
    }

    #[test]
    fn the_changes_name_what_the_store_has_to_keep() {
        let (mut library, id) = library_following_one();
        library.merge(id, document(vec![fetched("a", 10)]), 100);
        let changes = library.take_changes();
        assert!(changes.feeds);
        assert_eq!(changes.items, BTreeSet::from([id]));
        assert_eq!(library.take_changes(), Changes::default());

        library.unfollow(id);
        let changes = library.take_changes();
        assert!(changes.feeds && changes.items.is_empty());
        assert_eq!(changes.removed, BTreeSet::from([id]));
    }

    #[test]
    fn a_new_feed_takes_an_id_no_feed_followed_has() {
        let (mut library, first) = library_following_one();
        let second = library.follow("https://example.com/feed".to_owned(), "b".to_owned());
        assert_ne!(first, second);
        library.unfollow(first);
        let third = library.follow("https://example.net/feed".to_owned(), "c".to_owned());
        assert_ne!(third, second);
    }
}
