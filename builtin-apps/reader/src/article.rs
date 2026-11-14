//! An item's text as the app lays it out to read: a run of blocks, headings, paragraphs, list
//! items, quotations, code, pictures and rules, each paragraph a run of spans that are plain,
//! emphasised, strong, code or a link. What a feed's HTML is read into ([`crate::article_html`]),
//! and what the reading page draws; not HTML, and nothing here fetches or draws.

/// A stretch of a paragraph's text in one style.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) text: String,
    pub(crate) style: SpanStyle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SpanStyle {
    pub(crate) strong: bool,
    pub(crate) emphasised: bool,
    pub(crate) code: bool,
    /// Where it leads, for a link's text.
    pub(crate) link: Option<String>,
}

/// What a list item begins with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Marker {
    Bullet,
    Number(u32),
    /// Nothing: a later paragraph of an item whose first already showed its marker.
    Continued,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    /// A heading, from 1, the largest, to 6.
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    Paragraph(Vec<Span>),
    /// One item of a list, `depth` lists deep, from 1.
    ListItem {
        marker: Marker,
        depth: u8,
        spans: Vec<Span>,
    },
    Quote(Vec<Span>),
    /// Text set as written, line by line: code.
    Code(String),
    /// A picture at `address`, which `description` says in words.
    Picture {
        address: String,
        description: String,
    },
    /// What is said of the picture above it.
    Caption(Vec<Span>),
    Rule,
}

impl Block {
    /// Its words, where it has any.
    pub(crate) fn spans(&self) -> Option<&[Span]> {
        match self {
            Block::Heading { spans, .. }
            | Block::Paragraph(spans)
            | Block::ListItem { spans, .. }
            | Block::Quote(spans)
            | Block::Caption(spans) => Some(spans),
            Block::Code(_) | Block::Picture { .. } | Block::Rule => None,
        }
    }
}

/// The words of `blocks` as one line of plain text, no longer than `limit` characters, cut at a
/// word and ended with an ellipsis where they are longer: what a list shows beneath a title.
pub(crate) fn preview(blocks: &[Block], limit: usize) -> String {
    let text = blocks
        .iter()
        .filter(|block| !matches!(block, Block::Caption(_)))
        .filter_map(Block::spans)
        .map(text_of)
        .collect::<Vec<String>>()
        .join(" ");
    cut_at_a_word(&text, limit)
}

/// The text of `spans`, their styles aside.
pub(crate) fn text_of(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// `text` on one line, its first words up to `limit` characters, with an ellipsis where it is cut.
pub(crate) fn cut_at_a_word(text: &str, limit: usize) -> String {
    let mut cut = String::new();
    for word in text.split_whitespace() {
        if cut.chars().count() + word.chars().count() + 1 > limit {
            cut.push('…');
            break;
        }
        if !cut.is_empty() {
            cut.push(' ');
        }
        cut.push_str(word);
    }
    cut
}

/// Takes out of `blocks` the heading they begin with, first or after one line above it, where it
/// only says `title` again, as a page's own headline does above its text.
pub(crate) fn drop_repeated_title(blocks: &mut Vec<Block>, title: &str) {
    let headline = blocks
        .iter()
        .take(2)
        .position(|block| matches!(block, Block::Heading { .. }) && says(block, title));
    if let Some(index) = headline {
        blocks.remove(index);
    }
}

/// Whether `block`'s words are `title`, letter case aside.
fn says(block: &Block, title: &str) -> bool {
    block
        .spans()
        .is_some_and(|spans| text_of(spans).trim().eq_ignore_ascii_case(title.trim()))
}

/// The address of the first picture among `blocks`.
pub(crate) fn first_picture(blocks: &[Block]) -> Option<&str> {
    blocks.iter().find_map(|block| match block {
        Block::Picture { address, .. } => Some(address.as_str()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Span {
        Span {
            text: text.to_owned(),
            style: SpanStyle::default(),
        }
    }

    #[test]
    fn a_preview_is_the_first_words_cut_at_a_word() {
        let blocks = [
            Block::Heading {
                level: 2,
                spans: vec![plain("Launch day")],
            },
            Block::Picture {
                address: "https://example.org/a.png".to_owned(),
                description: "A rocket".to_owned(),
            },
            Block::Caption(vec![plain("Credit: someone")]),
            Block::Paragraph(vec![
                plain("The rocket left the "),
                plain("pad"),
                plain("."),
            ]),
        ];
        assert_eq!(preview(&blocks, 200), "Launch day The rocket left the pad.");
        assert_eq!(preview(&blocks, 22), "Launch day The rocket…");
        assert_eq!(first_picture(&blocks), Some("https://example.org/a.png"));
    }

    #[test]
    fn a_headline_that_says_the_title_again_is_left_out() {
        let headline = Block::Heading {
            level: 1,
            spans: vec![plain("Launch Day")],
        };
        let text = Block::Paragraph(vec![plain("The rocket left.")]);

        let mut blocks = vec![text.clone(), headline.clone(), text.clone()];
        drop_repeated_title(&mut blocks, "launch day ");
        assert_eq!(blocks, [text.clone(), text.clone()]);

        let mut other = vec![headline.clone(), text.clone()];
        drop_repeated_title(&mut other, "Another title");
        assert_eq!(other.len(), 2);

        let mut later = vec![text.clone(), text.clone(), headline];
        drop_repeated_title(&mut later, "Launch Day");
        assert_eq!(later.len(), 3, "a later heading is the text's own");
    }
}
