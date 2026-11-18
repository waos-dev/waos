//! waos.app's latest docs, for when a version's archive cannot be had: `llms-full.txt`, every
//! page one after the other, each starting `# <title> (https://waos.app/docs/<page>)`, and
//! `agents.md`, the playbook.
//!
//! The invariant: these are the site's, the latest release's, and are said to be: their origin is
//! `Latest`, whatever version they stand in for.

use super::page::{Page, SITE, ended};
use super::{Docs, Line, Origin, PLAYBOOK, PLAYBOOK_DESCRIPTION};

/// The docs `pages` (`llms-full.txt`) and `playbook` hold, standing in for `version`'s.
pub fn read(version: &str, pages: &str, playbook: &str) -> Docs {
    let mut docs = Docs {
        version: version.to_owned(),
        origin: Origin::Latest,
        pages: vec![Page::playbook(PLAYBOOK, PLAYBOOK_DESCRIPTION, playbook)],
        lines: vec![Line::Page {
            name: PLAYBOOK.to_owned(),
            depth: 0,
        }],
    };
    let mut current: Option<(String, String, Vec<&str>)> = None;
    for line in pages.lines() {
        if let Some((title, name)) = page_heading(line) {
            add(&mut docs, current.take());
            current = Some((name, title, Vec::new()));
        } else if let Some((_, _, body)) = current.as_mut() {
            body.push(line);
        }
    }
    add(&mut docs, current);
    docs
}

fn add(docs: &mut Docs, page: Option<(String, String, Vec<&str>)>) {
    let Some((name, title, body)) = page else {
        return;
    };
    let markdown = format!("# {title}\n\n{}", body.join("\n").trim());
    docs.lines.push(Line::Page {
        name: name.clone(),
        depth: name.matches('/').count(),
    });
    docs.pages.push(Page {
        name,
        title,
        description: None,
        markdown: ended(&markdown),
    });
}

/// The title and name of the page `line` starts, if it starts one.
fn page_heading(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("# ")?.strip_suffix(')')?;
    let (title, address) = rest.rsplit_once(" (")?;
    let name = address.strip_prefix(SITE)?.strip_prefix("/docs/")?;
    Some((title.to_owned(), name.to_owned()))
}
