//! Finding a site's feed from one of its pages: the feed a page names in its head, as
//! `<link rel="alternate" type="application/rss+xml" href="…">`, which is how a browser and a feed
//! reader find it when their user gives a site's address rather than its feed's.

use scraper::{Html, Selector};
use url::Url;

/// The types a page names a feed by.
const FEED_TYPES: [&str; 3] = [
    "application/atom+xml",
    "application/rss+xml",
    "application/feed+json",
];

/// The address of the first feed `page`, fetched from `address`, names; none where it names none.
pub(crate) fn feed_named_by(page: &[u8], address: &Url) -> Option<Url> {
    let page = Html::parse_document(&String::from_utf8_lossy(page));
    let links = Selector::parse("link[rel~=alternate][href]").ok()?;
    let named = page.select(&links).find(|link| {
        let kind = link.value().attr("type").unwrap_or_default();
        FEED_TYPES.contains(&kind.trim().to_ascii_lowercase().as_str())
    })?;
    let feed = address.join(named.value().attr("href")?.trim()).ok()?;
    matches!(feed.scheme(), "http" | "https").then_some(feed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_names_its_feed_in_its_head() {
        let address = Url::parse("https://example.org/blog/").unwrap();
        let page = br#"<html><head>
            <link rel="stylesheet" href="/style.css">
            <link rel="alternate" type="text/html" hreflang="fr" href="/fr/">
            <link rel="alternate" type="application/rss+xml" title="Posts" href="feed.xml">
            <link rel="alternate" type="application/atom+xml" href="/atom.xml">
            </head><body>Welcome</body></html>"#;
        let feed = feed_named_by(page, &address).unwrap();
        assert_eq!(feed.as_str(), "https://example.org/blog/feed.xml");
    }

    #[test]
    fn a_page_that_names_no_feed_has_none() {
        let address = Url::parse("https://example.org/").unwrap();
        assert_eq!(
            feed_named_by(b"<html><body>Hello</body></html>", &address),
            None
        );
    }
}
