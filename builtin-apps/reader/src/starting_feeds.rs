//! The feeds the app follows at its first launch, so that its lists show something before the
//! user adds a feed of their own: three public feeds their publishers offer to feed readers.

use crate::library::Library;

/// Each feed's address and what it is called until its document says.
const STARTING_FEEDS: [(&str, &str); 3] = [
    ("https://www.nasa.gov/feed/", "NASA"),
    ("https://blog.rust-lang.org/feed.xml", "Rust Blog"),
    (
        "https://en.wikipedia.org/w/api.php?action=featuredfeed&feed=featured&feedformat=atom",
        "Wikipedia featured articles",
    ),
];

/// The library a first launch starts with.
pub(crate) fn starting_library() -> Library {
    let mut library = Library::default();
    for (address, title) in STARTING_FEEDS {
        library.follow(address.to_owned(), title.to_owned());
    }
    library
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed_address::feed_address;

    #[test]
    fn the_starting_feeds_are_addresses_the_app_would_take() {
        let library = starting_library();
        assert_eq!(library.feeds().count(), STARTING_FEEDS.len());
        for feed in library.feeds() {
            assert_eq!(feed_address(&feed.address).unwrap().as_str(), feed.address);
        }
    }
}
