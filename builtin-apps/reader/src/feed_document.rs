//! Reading a feed's document, RSS or Atom as `feed-rs` parses them, into what the library keeps of
//! it: the feed's title, and each item's key, title, link, date, text, first words and picture.
//!
//! An item's picture is the one its feed names for it (a media thumbnail, or an attached image no
//! larger than a list's picture should cost to fetch), or else the first illustration in its text.

use feed_rs::model::{Entry, Link, MediaObject, Text};
use url::Url;

use crate::article::{self, Block};
use crate::article_html::{blocks_of, plain_text};
use crate::feed::{FetchedFeed, FetchedItem};

/// The most characters of an item's text a list shows beneath its title.
const PREVIEW_LENGTH: usize = 280;

/// The largest attached image taken as an item's picture, in bytes, where its feed says its size:
/// a feed of photographs attaches originals of many megabytes, which a list does not fetch.
const LARGEST_ATTACHED_PICTURE: u64 = 3_000_000;

/// What an item with no title is called.
const UNTITLED: &str = "Untitled";

/// Why a document is no feed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NotAFeed(pub(crate) String);

/// What `document`, fetched from `address`, says: its feed's title and items.
pub(crate) fn read(document: &[u8], address: &Url) -> Result<FetchedFeed, NotAFeed> {
    let parser = feed_rs::parser::Builder::new()
        .base_uri(Some(address.as_str()))
        .build();
    let feed = parser
        .parse(document)
        .map_err(|error| NotAFeed(error.to_string()))?;
    let title = feed
        .title
        .as_ref()
        .map(said)
        .filter(|title| !title.is_empty());
    let items = feed
        .entries
        .into_iter()
        .map(|entry| item_of(entry, address))
        .collect();
    Ok(FetchedFeed { title, items })
}

fn item_of(entry: Entry, feed_address: &Url) -> FetchedItem {
    let link = link_of(&entry.links, feed_address);
    let base = link.as_ref().unwrap_or(feed_address);
    let html = html_of(&entry);
    let blocks = blocks_of(&html, Some(base));
    let picture =
        named_picture(&entry.media).or_else(|| article::first_picture(&blocks).map(str::to_owned));
    let preview = preview_of(&entry, &blocks);
    FetchedItem {
        key: entry.id,
        title: title_of(entry.title.as_ref(), &blocks),
        link: link.map(String::from),
        published: entry.published.or(entry.updated).and_then(seconds_of),
        preview,
        html,
        picture,
    }
}

/// The first lines of an item's text: its feed's own summary of it where the feed gives one beside
/// the text, and the text's first words otherwise.
fn preview_of(entry: &Entry, blocks: &[Block]) -> String {
    let summary = entry
        .summary
        .as_ref()
        .filter(|_| entry.content.is_some())
        .map(said)
        .filter(|summary| !summary.is_empty());
    match summary {
        Some(summary) => article::cut_at_a_word(&summary, PREVIEW_LENGTH),
        None => article::preview(blocks, PREVIEW_LENGTH),
    }
}

/// An item's title; where its feed gives none, its first words, or [`UNTITLED`].
fn title_of(title: Option<&Text>, blocks: &[Block]) -> String {
    let title = title.map(said).unwrap_or_default();
    if !title.is_empty() {
        return title;
    }
    let first_words = article::preview(blocks, 80);
    if first_words.is_empty() {
        UNTITLED.to_owned()
    } else {
        first_words
    }
}

/// What `text` says, as plain text on one line.
fn said(text: &Text) -> String {
    plain_text(&text.content)
}

/// Where the item's original is: the link its feed calls the alternate, or the first it gives,
/// read against the feed's own address.
fn link_of(links: &[Link], feed_address: &Url) -> Option<Url> {
    let alternate = links
        .iter()
        .find(|link| matches!(link.rel.as_deref(), None | Some("alternate")));
    let link = alternate.or(links.first())?;
    let url = feed_address.join(link.href.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// The item's text as HTML: its content, or else its summary; plain text made paragraphs.
fn html_of(entry: &Entry) -> String {
    let content = entry.content.as_ref().and_then(|content| {
        let body = content.body.as_ref()?;
        Some((body, content.content_type.subty().as_str() == "plain"))
    });
    let summary = entry.summary.as_ref().map(|summary| {
        (
            &summary.content,
            summary.content_type.subty().as_str() == "plain",
        )
    });
    match content.or(summary) {
        Some((text, true)) => paragraphs_of(text),
        Some((html, false)) => html.clone(),
        None => String::new(),
    }
}

/// Plain `text` as HTML, a paragraph to each of its lines.
fn paragraphs_of(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("<p>{}</p>", escaped(line)))
        .collect()
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The picture the feed names for an item: a thumbnail, or an image attached to it.
fn named_picture(media: &[MediaObject]) -> Option<String> {
    let thumbnail = media
        .iter()
        .flat_map(|object| &object.thumbnails)
        .map(|thumbnail| thumbnail.image.uri.clone());
    let attached = media
        .iter()
        .flat_map(|object| &object.content)
        .filter(|content| {
            let is_image = content
                .content_type
                .as_ref()
                .is_some_and(|kind| kind.ty().as_str() == "image");
            is_image
                && content
                    .size
                    .is_none_or(|size| size <= LARGEST_ATTACHED_PICTURE)
        })
        .filter_map(|content| content.url.as_ref().map(|url| url.to_string()));
    thumbnail
        .chain(attached)
        .find(|address| address.starts_with("https://") || address.starts_with("http://"))
}

/// `moment` in seconds since 1970; none for a moment before it.
fn seconds_of(moment: chrono::DateTime<chrono::Utc>) -> Option<u64> {
    u64::try_from(moment.timestamp()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address() -> Url {
        Url::parse("https://example.org/blog/feed.xml").unwrap()
    }

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Example &amp; Co</title>
  <entry>
    <title>First post</title>
    <link rel="alternate" href="/blog/first/"/>
    <id>tag:example.org,2026:first</id>
    <updated>2026-11-12T00:00:00+00:00</updated>
    <content type="html">&lt;p&gt;Hello &lt;a href="more"&gt;there&lt;/a&gt;.&lt;/p&gt;&lt;img src="hello.png" alt="A wave"&gt;</content>
  </entry>
</feed>"#;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
<channel>
  <title>Pictures</title>
  <link>https://example.org/</link>
  <item>
    <title>A large photograph</title>
    <link>https://example.org/photo/1</link>
    <description>Taken from orbit.</description>
    <enclosure url="https://example.org/original.jpg" length="8498785" type="image/jpeg"/>
    <guid isPermaLink="false">photo-1</guid>
    <pubDate>Thu, 12 Nov 2026 05:00:00 +0000</pubDate>
  </item>
  <item>
    <link>https://example.org/photo/2</link>
    <description>&lt;p&gt;No title here, only words.&lt;/p&gt;</description>
    <media:content url="https://example.org/full-2.jpg" type="image/jpeg">
      <media:thumbnail url="https://example.org/thumb-2.jpg"/>
    </media:content>
    <guid>https://example.org/photo/2</guid>
  </item>
</channel>
</rss>"#;

    #[test]
    fn an_atom_entry_becomes_an_item_with_its_addresses_in_full() {
        let feed = read(ATOM.as_bytes(), &address()).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Example & Co"));
        let [item] = feed.items.as_slice() else {
            panic!("one item");
        };
        assert_eq!(item.key, "tag:example.org,2026:first");
        assert_eq!(item.title, "First post");
        assert_eq!(
            item.link.as_deref(),
            Some("https://example.org/blog/first/")
        );
        assert_eq!(item.published, Some(1_794_441_600));
        assert_eq!(item.preview, "Hello there.");
        assert_eq!(
            item.picture.as_deref(),
            Some("https://example.org/blog/first/hello.png")
        );
    }

    #[test]
    fn an_rss_item_takes_its_feeds_picture_unless_it_is_an_original_too_large() {
        let feed = read(RSS.as_bytes(), &address()).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Pictures"));
        let [large, small] = feed.items.as_slice() else {
            panic!("two items");
        };
        assert_eq!(large.key, "photo-1");
        assert_eq!(large.picture, None);
        assert_eq!(large.preview, "Taken from orbit.");
        assert_eq!(small.title, "No title here, only words.");
        assert_eq!(
            small.picture.as_deref(),
            Some("https://example.org/thumb-2.jpg")
        );
        assert_eq!(small.published, None);
    }

    #[test]
    fn a_page_that_is_no_feed_is_refused() {
        let page = b"<!doctype html><html><body>Not a feed</body></html>";
        assert!(read(page, &address()).is_err());
    }
}
