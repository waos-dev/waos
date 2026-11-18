//! The docs' map, in two forms: the terminal's, which `cargo waos docs` prints, a line a page by
//! the name it takes, and the skill's `docs/index.md`, the same lines as links to the files.
//!
//! The invariant: both follow [`Docs::lines`], so the map and the skill's index list the same
//! pages in the same order, and each says how a page's links are followed.

use super::{Docs, Line, PLAYBOOK, Page};

/// The map `cargo waos docs` prints: how to read on, then a line for each page, indented under
/// its section, with its name, its title and its description.
pub fn terminal_map(docs: &Docs) -> String {
    let width = docs
        .lines()
        .iter()
        .filter_map(|line| match line {
            Line::Page { name, depth } => Some(depth * 2 + name.len()),
            Line::Heading { .. } => None,
        })
        .max()
        .unwrap_or(0);
    let mut text = format!(
        "WaOS's docs, waos-cli {}'s. `cargo waos docs <page>` prints a page, `cargo waos docs --search <text>` prints the lines that hold the text, and a link to /docs/<page> in a page is `cargo waos docs <page>`.\n\n",
        env!("CARGO_PKG_VERSION")
    );
    for line in docs.lines() {
        match line {
            Line::Heading { title, depth } => {
                text.push_str(&format!("{}{title}\n", indent(*depth)));
            }
            Line::Page { name, depth } => {
                let page = page_named(docs, name);
                let padded = width - depth * 2;
                text.push_str(&format!(
                    "{}{name:<padded$}  {}\n",
                    indent(*depth),
                    summary(page)
                ));
            }
        }
    }
    text
}

/// The skill's `docs/index.md`: the map as a list of links, each page's file beside the index,
/// the playbook the skill's own `SKILL.md`.
pub fn skill_index(docs: &Docs) -> String {
    let mut text = format!(
        "# WaOS's docs\n\nwaos-cli {}'s, a page a file. A link to /docs/<page> in a page is the file <page>.md in this folder.\n\n",
        env!("CARGO_PKG_VERSION")
    );
    for line in docs.lines() {
        match line {
            Line::Heading { title, depth } => {
                text.push_str(&format!("{}- **{title}**\n", indent(*depth)));
            }
            Line::Page { name, depth } => {
                let page = page_named(docs, name);
                let file = if name == PLAYBOOK {
                    "../SKILL.md".to_owned()
                } else {
                    format!("{name}.md")
                };
                let description = page
                    .description
                    .as_deref()
                    .map(|description| format!(": {description}"))
                    .unwrap_or_default();
                text.push_str(&format!(
                    "{}- [{}]({file}){description}\n",
                    indent(*depth),
                    page.title
                ));
            }
        }
    }
    text
}

fn page_named<'a>(docs: &'a Docs, name: &str) -> &'a Page {
    docs.page(name).expect("each of the map's pages is a page")
}

/// A page's title, then its description.
fn summary(page: &Page) -> String {
    match &page.description {
        Some(description) => format!("{}: {description}", page.title),
        None => page.title.clone(),
    }
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}
