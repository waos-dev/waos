//! Where the library is kept between runs, in the program's `/data`: `feeds.json`, the feeds the
//! user follows; `items/<feed id>.json`, the items kept of each, a file to a feed so that a
//! refresh or a read mark writes only its feed's; and `settings.json`, what the user chose.
//!
//! The invariant a later editor will break: every file is replaced whole, through `safe_write`,
//! so a write that fails leaves what was kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::feed::{Feed, FeedId, Item};
use crate::library::{Changes, Followed, Library};
use crate::safe_write::write_safely;
use crate::settings::Settings;

/// The program's own folder, which WaOS keeps for it.
const DATA: &str = "/data";

const FEEDS: &str = "feeds.json";
const ITEMS: &str = "items";
const SETTINGS: &str = "settings.json";

#[derive(Clone, Debug)]
pub(crate) struct Store {
    folder: PathBuf,
}

impl Store {
    /// The store in the program's `/data`.
    pub(crate) fn in_data() -> Store {
        Store::at(PathBuf::from(DATA))
    }

    /// The store in `folder`.
    pub(crate) fn at(folder: PathBuf) -> Store {
        Store { folder }
    }

    /// The feeds kept, each with its items; none where no library was kept yet, at the first
    /// launch. A feed whose items cannot be read is kept with none, to be fetched again.
    pub(crate) fn load(&self) -> io::Result<Option<Vec<Followed>>> {
        let path = self.folder.join(FEEDS);
        if !path.exists() {
            return Ok(None);
        }
        let feeds: Vec<Feed> = read_json(&path)?;
        let followed = feeds.into_iter().map(|feed| self.with_items(feed));
        Ok(Some(followed.collect()))
    }

    /// Keeps what `changes` names of `library`.
    pub(crate) fn keep(&self, library: &Library, changes: &Changes) -> io::Result<()> {
        fs::create_dir_all(self.folder.join(ITEMS))?;
        for id in &changes.removed {
            remove_if_there(&self.items_path(*id))?;
        }
        for id in &changes.items {
            if let Some(followed) = library.followed(*id) {
                write_json(&self.items_path(*id), &followed.items)?;
            }
        }
        if changes.feeds {
            let feeds: Vec<&Feed> = library.feeds().collect();
            write_json(&self.folder.join(FEEDS), &feeds)?;
        }
        Ok(())
    }

    /// What the user chose; the defaults where nothing was kept, or it cannot be read.
    pub(crate) fn load_settings(&self) -> Settings {
        read_json(&self.folder.join(SETTINGS)).unwrap_or_default()
    }

    pub(crate) fn keep_settings(&self, settings: Settings) -> io::Result<()> {
        fs::create_dir_all(&self.folder)?;
        write_json(&self.folder.join(SETTINGS), &settings)
    }

    fn with_items(&self, feed: Feed) -> Followed {
        let path = self.items_path(feed.id);
        let items: Vec<Item> = read_json(&path).unwrap_or_else(|error| {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!("the items kept in {} are not read: {error}", path.display());
            }
            Vec::new()
        });
        Followed { feed, items }
    }

    fn items_path(&self, id: FeedId) -> PathBuf {
        self.folder.join(ITEMS).join(format!("{}.json", id.0))
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    let json = serde_json::to_vec(value).map_err(io::Error::other)?;
    write_safely(path, &json)
}

fn remove_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::{FetchedFeed, FetchedItem};
    use crate::library::{ItemRef, Scope};

    fn item(key: &str) -> FetchedItem {
        FetchedItem {
            key: key.to_owned(),
            title: key.to_uppercase(),
            link: None,
            published: Some(7),
            html: "<p>Text</p>".to_owned(),
            preview: "Text".to_owned(),
            picture: Some("https://example.org/a.png".to_owned()),
        }
    }

    fn keep_changes(store: &Store, library: &mut Library) {
        let changes = library.take_changes();
        store.keep(library, &changes).unwrap();
    }

    fn kept_again(store: &Store) -> Library {
        Library::of(store.load().unwrap().unwrap())
    }

    #[test]
    fn nothing_is_kept_before_the_first_launch() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(folder.path().to_owned());
        assert!(store.load().unwrap().is_none());
        assert_eq!(store.load_settings(), Settings::default());
    }

    #[test]
    fn feeds_items_and_read_marks_come_back() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(folder.path().to_owned());
        let mut library = Library::default();
        let id = library.follow("https://example.org/feed".to_owned(), "Example".to_owned());
        let document = FetchedFeed {
            title: None,
            items: vec![item("a"), item("b")],
        };
        library.merge(id, document, 100);
        let a = ItemRef {
            feed: id,
            key: "a".to_owned(),
        };
        library.mark(&a, true);
        keep_changes(&store, &mut library);

        let again = kept_again(&store);
        assert_eq!(again.followed(id), library.followed(id));
        assert_eq!(again.unread(Scope::All), 1);
    }

    #[test]
    fn a_feed_unfollowed_leaves_no_items_behind() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(folder.path().to_owned());
        let mut library = Library::default();
        let id = library.follow("https://example.org/feed".to_owned(), "Example".to_owned());
        let document = FetchedFeed {
            title: None,
            items: vec![item("a")],
        };
        library.merge(id, document, 100);
        keep_changes(&store, &mut library);
        assert!(store.items_path(id).exists());

        library.unfollow(id);
        keep_changes(&store, &mut library);
        assert!(!store.items_path(id).exists());
        assert_eq!(kept_again(&store).feeds().count(), 0);
    }

    #[test]
    fn settings_come_back() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::at(folder.path().to_owned());
        let chosen = Settings {
            notify_of_new_items: true,
        };
        store.keep_settings(chosen).unwrap();
        assert_eq!(store.load_settings(), chosen);
    }
}
