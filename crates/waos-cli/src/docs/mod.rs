//! The docs `cargo waos` carries: this crate's `playbook.md` and `docs/`, which the website
//! renders too, and the reference's pages, which `build.rs` makes from the WIT and builds in with
//! the rest, so an agent reads the docs of the version it builds with, offline. `cargo waos docs` prints their map,
//! one page, or the lines that match a search; `cargo waos skill install` writes them as files.
//!
//! The invariant: a page is named as the site's address names it, less `/docs/`
//! (`guide/network` for `/docs/guide/network`), so a link in one page is the name of another;
//! the playbook, `/agents.md` on the site, is `playbook`.

mod front_matter;
mod map;
mod outline;
mod page;

use std::collections::BTreeMap;

use anyhow::{Result, bail};

pub use map::{skill_index, terminal_map};
pub use outline::Line;
pub use page::{Page, SITE};

/// Every file of the docs, `playbook.md` and the pages under `docs/`, by its path, as `build.rs`
/// built them in.
const BUNDLED: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/docs.rs"));

/// The playbook's name, and the line the map gives it.
pub const PLAYBOOK: &str = "playbook";
const PLAYBOOK_DESCRIPTION: &str = "the steps, from the tools to a packaged app";

/// The docs: their pages in the map's order, the playbook first, and the map's lines.
pub struct Docs {
    pages: Vec<Page>,
    lines: Vec<Line>,
}

/// A line of a page that matched a search: the page's name, the line's number in the page as
/// it prints, and the line.
#[derive(Debug, PartialEq, Eq)]
pub struct Hit {
    pub page: String,
    pub line: usize,
    pub text: String,
}

impl Docs {
    /// The docs built into this `cargo waos`.
    pub fn bundled() -> Docs {
        Docs::from_files(BUNDLED)
    }

    /// The docs `files` hold, each by its path from the crate's folder: `playbook.md`, the
    /// playbook, and the site's pages under `docs/`.
    pub fn from_files(files: &[(&str, &str)]) -> Docs {
        let files: BTreeMap<&str, &str> = files.iter().copied().collect();
        let mut pages = Vec::new();
        let mut lines = Vec::new();
        if let Some(playbook) = files.get("playbook.md") {
            pages.push(Page::from_playbook(
                PLAYBOOK,
                PLAYBOOK_DESCRIPTION,
                playbook,
            ));
            lines.push(Line::Page {
                name: PLAYBOOK.to_owned(),
                depth: 0,
            });
        }
        let (site_pages, site_lines) = outline::read(&files, 0);
        pages.extend(site_pages);
        lines.extend(site_lines);
        Docs { pages, lines }
    }

    /// The pages, in the map's order.
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// The map's lines, headings and pages, in order.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The page `asked` names: its name (`guide/network`), its address on the site
    /// (`/docs/guide/network.md`, `https://waos.app/docs/waft#state`, `/agents.md`), or the last
    /// part of its name where one page alone ends so (`network`), or one page outside the
    /// reference does (`windows`, which is a guide page and an interface's).
    pub fn find(&self, asked: &str) -> Result<&Page> {
        let name = name_asked(asked);
        if let Some(page) = self.page(&name) {
            return Ok(page);
        }
        let ending: Vec<&Page> = self
            .pages
            .iter()
            .filter(|page| page.name.rsplit('/').next() == Some(name.as_str()))
            .collect();
        let written: Vec<&Page> = ending
            .iter()
            .copied()
            .filter(|page| !page.name.starts_with("reference/"))
            .collect();
        if let [page] = written.as_slice() {
            return Ok(page);
        }
        match ending.as_slice() {
            [page] => return Ok(page),
            [_, ..] => bail!(
                "{asked} is the end of several pages' names: {}",
                names(&ending)
            ),
            [] => {}
        }
        let close: Vec<&Page> = self
            .pages
            .iter()
            .filter(|page| page.name.contains(&name) || name.contains(&page.name))
            .take(5)
            .collect();
        if close.is_empty() {
            bail!("no page is named {asked}; `cargo waos docs` lists them");
        }
        bail!(
            "no page is named {asked}; perhaps {}. `cargo waos docs` lists them",
            names(&close)
        )
    }

    /// Each line of each page, as it prints, that holds `text`, whatever its case; pages in the
    /// map's order.
    pub fn search(&self, text: &str) -> Vec<Hit> {
        let wanted = text.to_lowercase();
        self.pages
            .iter()
            .flat_map(|page| {
                let markdown = page.markdown();
                markdown
                    .lines()
                    .enumerate()
                    .filter(|(_, line)| line.to_lowercase().contains(&wanted))
                    .map(|(index, line)| Hit {
                        page: page.name.clone(),
                        line: index + 1,
                        text: line.trim().to_owned(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn page(&self, name: &str) -> Option<&Page> {
        self.pages.iter().find(|page| page.name == name)
    }
}

/// The name a request for a page means: without the site's origin, `/docs/`, an anchor, a
/// trailing `.md` or `/`; the site's `agents.md` is the playbook.
fn name_asked(asked: &str) -> String {
    let mut name = asked.trim();
    name = name.strip_prefix(SITE).unwrap_or(name);
    name = name.split('#').next().unwrap_or(name);
    name = name.trim_start_matches('/');
    name = name.strip_prefix("docs/").unwrap_or(name);
    name = name.trim_end_matches('/');
    name = name
        .strip_suffix(".mdx")
        .or_else(|| name.strip_suffix(".md"))
        .unwrap_or(name);
    if name == "agents" {
        return PLAYBOOK.to_owned();
    }
    name.to_owned()
}

fn names(pages: &[&Page]) -> String {
    let named: Vec<&str> = pages.iter().map(|page| page.name.as_str()).collect();
    named.join(", ")
}

#[cfg(test)]
mod tests;
