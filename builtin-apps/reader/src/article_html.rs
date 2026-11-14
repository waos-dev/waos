//! Reading a feed item's HTML into the blocks the app lays out ([`crate::article`]): headings,
//! paragraphs, lists, quotations, code and pictures, with links, emphasis and strong text inside a
//! paragraph. Not a browser: nothing of the HTML's own styling, scripts, forms, tables' grids or
//! embedded frames is kept, a table's cells are read as lines of text, and what a page's chrome
//! puts around the text (navigation, buttons) is left out.
//!
//! The invariant a later editor will break: every address that leaves here, a link's or a
//! picture's, is absolute and `http` or `https`, resolved against the item's own address, so
//! nothing later fetches or opens anything else.

use std::collections::HashSet;

use scraper::node::{Element, Node};
use scraper::{ElementRef, Html};
use url::Url;

use crate::article::{Block, Marker, Span, SpanStyle};

/// Elements whose content is not the item's text.
const LEFT_OUT: [&str; 17] = [
    "script", "style", "nav", "button", "svg", "iframe", "form", "noscript", "head", "template",
    "select", "textarea", "video", "audio", "object", "canvas", "math",
];

/// A picture no larger than this on a side, where its HTML says its size, is an icon or a
/// tracker, not an illustration, and is left out.
const SMALLEST_PICTURE: u32 = 48;

/// The blocks of `html`, an item's text, whose relative addresses are read against `base`.
pub(crate) fn blocks_of(html: &str, base: Option<&Url>) -> Vec<Block> {
    let fragment = Html::parse_fragment(html);
    let mut reading = Reading::new(base);
    reading.read_children(fragment.root_element(), &SpanStyle::default());
    reading.end_block();
    reading.blocks
}

/// `html` as plain text on one line: what a title that carries markup says.
pub(crate) fn plain_text(html: &str) -> String {
    let fragment = Html::parse_fragment(html);
    let text: String = fragment.root_element().text().collect();
    text.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// What the words being read belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Paragraph,
    Heading(u8),
    Caption,
}

/// A list being read: what its items begin with.
#[derive(Clone, Copy, Debug)]
enum List {
    Bulleted,
    /// Numbered, with the number of its next item.
    Numbered(u32),
}

struct Reading<'a> {
    base: Option<&'a Url>,
    blocks: Vec<Block>,
    /// The spans of the block being read.
    spans: Vec<Span>,
    kind: Kind,
    /// The lists the words being read are inside, the innermost last.
    lists: Vec<List>,
    /// The marker of the list item being read, until its first block shows it.
    marker: Option<Marker>,
    /// Whether the words being read are inside a list's item.
    in_item: bool,
    quotes: u32,
    /// The pictures read so far, each by its address without its query and by what it is said to
    /// show: a page often gives one picture twice, at two sizes.
    pictures: HashSet<String>,
}

impl<'a> Reading<'a> {
    fn new(base: Option<&'a Url>) -> Reading<'a> {
        Reading {
            base,
            blocks: Vec::new(),
            spans: Vec::new(),
            kind: Kind::Paragraph,
            lists: Vec::new(),
            marker: None,
            in_item: false,
            quotes: 0,
            pictures: HashSet::new(),
        }
    }

    fn read_children(&mut self, parent: ElementRef<'_>, style: &SpanStyle) {
        for child in parent.children() {
            match child.value() {
                Node::Text(text) => self.read_text(text, style),
                Node::Element(_) => {
                    if let Some(element) = ElementRef::wrap(child) {
                        self.read_element(element, style);
                    }
                }
                _ => {}
            }
        }
    }

    fn read_element(&mut self, element: ElementRef<'_>, style: &SpanStyle) {
        let value = element.value();
        match value.name() {
            name if LEFT_OUT.contains(&name) => {}
            "br" => self.break_line(style),
            "hr" => self.read_rule(),
            "img" => self.read_picture(value),
            "pre" => self.read_code(element),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.read_heading(element, style),
            "figcaption" => self.read_as(Kind::Caption, element, style),
            "ul" | "menu" => self.read_list(List::Bulleted, element, style),
            "ol" => self.read_list(List::Numbered(first_number(value)), element, style),
            "li" => self.read_item(element, style),
            "blockquote" => self.read_quote(element, style),
            "td" | "th" => self.read_cell(element, style),
            name if is_block(name) => self.read_as(Kind::Paragraph, element, style),
            _ => self.read_children(element, &self.styled(value, style)),
        }
    }

    /// `style` with what the inline element `element` adds: strong, emphasis, code, or its link.
    fn styled(&self, element: &Element, style: &SpanStyle) -> SpanStyle {
        let mut style = style.clone();
        match element.name() {
            "b" | "strong" => style.strong = true,
            "i" | "em" | "cite" | "dfn" | "var" => style.emphasised = true,
            "code" | "kbd" | "samp" | "tt" => style.code = true,
            "a" => {
                let link = element.attr("href").and_then(|href| self.absolute(href));
                style.link = link.or(style.link);
            }
            _ => {}
        }
        style
    }

    /// Adds `text` to the block being read, its runs of white space each one space.
    fn read_text(&mut self, text: &str, style: &SpanStyle) {
        let mut said = String::new();
        let mut after_space = self.ends_with_space();
        for character in text.chars() {
            if !character.is_whitespace() {
                said.push(character);
                after_space = false;
            } else if !after_space {
                said.push(' ');
                after_space = true;
            }
        }
        self.say(said, style);
    }

    /// Whether a space now would follow another, or begin the block.
    fn ends_with_space(&self) -> bool {
        self.spans
            .last()
            .is_none_or(|span| span.text.ends_with([' ', '\n']))
    }

    fn break_line(&mut self, style: &SpanStyle) {
        if !self.spans.is_empty() {
            self.say("\n".to_owned(), style);
        }
    }

    /// Adds `text` in `style` to the block being read, joined to the span before it where that
    /// has the same style.
    fn say(&mut self, text: String, style: &SpanStyle) {
        if text.is_empty() {
            return;
        }
        match self.spans.last_mut() {
            Some(last) if last.style == *style => last.text.push_str(&text),
            _ => self.spans.push(Span {
                text,
                style: style.clone(),
            }),
        }
    }

    /// Ends the block being read: its words, where it has any, become a block of its kind.
    fn end_block(&mut self) {
        let spans = trimmed(std::mem::take(&mut self.spans));
        if spans.is_empty() {
            return;
        }
        let block = match self.kind {
            Kind::Heading(level) => Block::Heading { level, spans },
            Kind::Caption => Block::Caption(spans),
            Kind::Paragraph if self.in_item => Block::ListItem {
                marker: self.marker.take().unwrap_or(Marker::Continued),
                depth: u8::try_from(self.lists.len()).unwrap_or(u8::MAX).max(1),
                spans,
            },
            Kind::Paragraph if self.quotes > 0 => Block::Quote(spans),
            Kind::Paragraph => Block::Paragraph(spans),
        };
        self.blocks.push(block);
    }

    /// Reads `element` as a block of its own, of `kind`.
    fn read_as(&mut self, kind: Kind, element: ElementRef<'_>, style: &SpanStyle) {
        self.end_block();
        let outer = std::mem::replace(&mut self.kind, kind);
        self.read_children(element, style);
        self.end_block();
        self.kind = outer;
    }

    fn read_heading(&mut self, element: ElementRef<'_>, style: &SpanStyle) {
        let level = element.value().name()[1..].parse().unwrap_or(6);
        self.read_as(Kind::Heading(level), element, style);
    }

    fn read_list(&mut self, list: List, element: ElementRef<'_>, style: &SpanStyle) {
        self.end_block();
        self.lists.push(list);
        self.read_children(element, style);
        self.end_block();
        self.lists.pop();
    }

    fn read_item(&mut self, element: ElementRef<'_>, style: &SpanStyle) {
        self.end_block();
        let (outer_marker, outer_in_item) = (self.marker.take(), self.in_item);
        self.marker = Some(self.next_marker());
        self.in_item = true;
        self.read_children(element, style);
        self.end_block();
        self.marker = outer_marker;
        self.in_item = outer_in_item;
    }

    /// The marker of the next item of the innermost list; a bullet for an item outside any list.
    fn next_marker(&mut self) -> Marker {
        match self.lists.last_mut() {
            Some(List::Numbered(next)) => {
                let number = *next;
                *next += 1;
                Marker::Number(number)
            }
            _ => Marker::Bullet,
        }
    }

    fn read_quote(&mut self, element: ElementRef<'_>, style: &SpanStyle) {
        self.end_block();
        self.quotes += 1;
        self.read_children(element, style);
        self.end_block();
        self.quotes -= 1;
    }

    /// Reads a table's cell as words of its row's line, apart from the next cell's.
    fn read_cell(&mut self, element: ElementRef<'_>, style: &SpanStyle) {
        self.read_children(element, style);
        self.read_text("  ", style);
    }

    fn read_rule(&mut self) {
        self.end_block();
        self.blocks.push(Block::Rule);
    }

    fn read_code(&mut self, element: ElementRef<'_>) {
        self.end_block();
        let code: String = element.text().collect();
        let code = code.trim_matches('\n').to_owned();
        if !code.trim().is_empty() {
            self.blocks.push(Block::Code(code));
        }
    }

    /// Reads a picture as a block of its own, where it is an illustration with an address.
    fn read_picture(&mut self, picture: &Element) {
        if is_small(picture) {
            return;
        }
        let source = picture.attr("src").or_else(|| picture.attr("data-src"));
        let Some(address) = source.and_then(|source| self.absolute(source)) else {
            return;
        };
        let description = picture.attr("alt").unwrap_or_default().trim().to_owned();
        if self.was_read_already(&address, &description) {
            return;
        }
        self.end_block();
        self.blocks.push(Block::Picture {
            address,
            description,
        });
    }

    /// Whether a picture at `address`, its query aside, or one said to show `description`, was
    /// read before; notes both as read.
    fn was_read_already(&mut self, address: &str, description: &str) -> bool {
        let without_query = address.split(['?', '#']).next().unwrap_or(address);
        let new_address = self.pictures.insert(without_query.to_owned());
        let new_description =
            description.is_empty() || self.pictures.insert(format!("alt:{description}"));
        !(new_address && new_description)
    }

    /// `address` in full, read against the item's own; none unless it is `http` or `https`.
    fn absolute(&self, address: &str) -> Option<String> {
        let address = address.trim();
        let url = match self.base {
            Some(base) => base.join(address),
            None => Url::parse(address),
        }
        .ok()?;
        matches!(url.scheme(), "http" | "https").then(|| url.into())
    }
}

/// Whether `name` is an element that holds a block of text of its own.
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "aside"
            | "main"
            | "header"
            | "footer"
            | "figure"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "caption"
            | "dl"
            | "dt"
            | "dd"
            | "details"
            | "summary"
            | "address"
            | "center"
    )
}

/// The number an ordered list's first item takes.
fn first_number(list: &Element) -> u32 {
    list.attr("start")
        .and_then(|start| start.trim().parse().ok())
        .unwrap_or(1)
}

/// Whether `picture` says it is no larger than [`SMALLEST_PICTURE`] on a side.
fn is_small(picture: &Element) -> bool {
    ["width", "height"].into_iter().any(|side| {
        picture
            .attr(side)
            .and_then(|length| length.trim().parse::<u32>().ok())
            .is_some_and(|length| length <= SMALLEST_PICTURE)
    })
}

/// `spans` without the white space before the first word and after the last, and without the
/// spans that leaves empty.
fn trimmed(mut spans: Vec<Span>) -> Vec<Span> {
    if let Some(last) = spans.last_mut() {
        last.text.truncate(last.text.trim_end().len());
    }
    if let Some(first) = spans.first_mut() {
        first.text = first.text.trim_start().to_owned();
    }
    spans.retain(|span| !span.text.is_empty());
    spans
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

    fn read(html: &str) -> Vec<Block> {
        let base = Url::parse("https://example.org/posts/one/").unwrap();
        blocks_of(html, Some(&base))
    }

    #[test]
    fn paragraphs_and_headings_are_blocks_with_their_white_space_folded() {
        let blocks = read("<h2> Launch\n day </h2>\n<p>The   rocket\nleft.</p><p></p>loose words");
        assert_eq!(
            blocks,
            [
                Block::Heading {
                    level: 2,
                    spans: vec![plain("Launch day")]
                },
                Block::Paragraph(vec![plain("The rocket left.")]),
                Block::Paragraph(vec![plain("loose words")]),
            ]
        );
    }

    #[test]
    fn a_paragraphs_spans_keep_their_styles_and_links() {
        let blocks = read(
            r#"<p>See <a href="../two/">the <em>next</em> post</a>, <strong>now</strong> with <code>cargo</code>.</p>"#,
        );
        let link = SpanStyle {
            link: Some("https://example.org/posts/two/".to_owned()),
            ..SpanStyle::default()
        };
        let spans = vec![
            plain("See "),
            Span {
                text: "the ".to_owned(),
                style: link.clone(),
            },
            Span {
                text: "next".to_owned(),
                style: SpanStyle {
                    emphasised: true,
                    ..link.clone()
                },
            },
            Span {
                text: " post".to_owned(),
                style: link,
            },
            plain(", "),
            Span {
                text: "now".to_owned(),
                style: SpanStyle {
                    strong: true,
                    ..SpanStyle::default()
                },
            },
            plain(" with "),
            Span {
                text: "cargo".to_owned(),
                style: SpanStyle {
                    code: true,
                    ..SpanStyle::default()
                },
            },
            plain("."),
        ];
        assert_eq!(blocks, [Block::Paragraph(spans)]);
    }

    #[test]
    fn lists_number_their_items_and_nest() {
        let blocks = read(
            "<ol start=3><li>three</li><li>four<ul><li>inner</li></ul></li></ol><ul><li><p>one</p><p>more</p></li></ul>",
        );
        let item = |marker, depth, text: &str| Block::ListItem {
            marker,
            depth,
            spans: vec![plain(text)],
        };
        assert_eq!(
            blocks,
            [
                item(Marker::Number(3), 1, "three"),
                item(Marker::Number(4), 1, "four"),
                item(Marker::Bullet, 2, "inner"),
                item(Marker::Bullet, 1, "one"),
                item(Marker::Continued, 1, "more"),
            ]
        );
    }

    #[test]
    fn quotations_code_rules_and_captions_are_blocks_of_their_own() {
        let blocks = read(
            "<blockquote><p>Said so.</p></blockquote><pre><code>fn main() {\n    run();\n}\n</code></pre><hr><figure><img src=\"/a.png\" alt=\" A rocket \"><figcaption>Credit: someone</figcaption></figure>",
        );
        assert_eq!(
            blocks,
            [
                Block::Quote(vec![plain("Said so.")]),
                Block::Code("fn main() {\n    run();\n}".to_owned()),
                Block::Rule,
                Block::Picture {
                    address: "https://example.org/a.png".to_owned(),
                    description: "A rocket".to_owned()
                },
                Block::Caption(vec![plain("Credit: someone")]),
            ]
        );
    }

    #[test]
    fn a_pages_chrome_trackers_and_unsafe_addresses_are_left_out() {
        let blocks = read(
            r#"<nav><ul><li>Home</li></ul></nav><button>Menu</button><script>track()</script>
            <img src="https://example.org/pixel.gif" width="1" height="1">
            <img src="data:image/png;base64,AAAA"><p><a href="javascript:alert(1)">words</a></p>
            <img src="//cdn.example.org/b.jpg">"#,
        );
        assert_eq!(
            blocks,
            [
                Block::Paragraph(vec![plain("words")]),
                Block::Picture {
                    address: "https://cdn.example.org/b.jpg".to_owned(),
                    description: String::new()
                },
            ]
        );
    }

    #[test]
    fn a_picture_given_twice_is_read_once() {
        let blocks = read(
            r#"<img src="/a.jpg?w=1536" alt="A rocket"><img src="/a.jpg?w=2048" alt="A rocket">
            <img src="/b.jpg"><img src="/large/a.jpg" alt="A rocket"><img src="/c.jpg">"#,
        );
        let addresses: Vec<&str> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Picture { address, .. } => Some(address.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            addresses,
            [
                "https://example.org/a.jpg?w=1536",
                "https://example.org/b.jpg",
                "https://example.org/c.jpg"
            ]
        );
    }

    #[test]
    fn a_line_break_stays_in_its_paragraph_and_a_table_reads_row_by_row() {
        let blocks = read(
            "<p>one<br>two</p><table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>",
        );
        assert_eq!(
            blocks,
            [
                Block::Paragraph(vec![plain("one\ntwo")]),
                Block::Paragraph(vec![plain("a b")]),
                Block::Paragraph(vec![plain("c")]),
            ]
        );
    }

    #[test]
    fn a_title_with_markup_reads_as_plain_text() {
        assert_eq!(
            plain_text("Rust &amp; <i>friends</i>\n 2026"),
            "Rust & friends 2026"
        );
    }
}
