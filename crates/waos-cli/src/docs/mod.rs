//! The docs `cargo waos docs` and `cargo waos docs skill install` serve: those of the version the app
//! builds with, from the archive its release carries (`waos-docs-<version>.tar.gz`, from
//! download.waos.app or else GitHub, which
//! the site's `write-docs-bundle.mjs` makes at the release's tag), downloaded once into the user's
//! cache and read from there offline afterwards; or, where none could be downloaded, waos.app's
//! latest, said to be so.
//!
//! The invariant: a page is named as the site's address names it, less `/docs/` (`guide/network`
//! for `/docs/guide/network`), so a link in one page is the name of another; the playbook,
//! `/agents.md` on the site, is `playbook`. The order and the pages are the archive's: nothing
//! here reads the site's sources.

mod bundle;
mod cache;
mod download;
mod latest;
mod map;
mod page;
mod version;

use std::path::Path;

use anyhow::{Result, bail};

pub use map::{skill_index, terminal_map, whose};
pub use page::{Page, SITE};

/// The playbook's name, and the line the map gives it.
pub const PLAYBOOK: &str = "playbook";
const PLAYBOOK_DESCRIPTION: &str = "the steps, from the tools to a packaged app";

/// The docs: their version, where they came from, their pages in the map's order, the playbook
/// first, and the map's lines.
pub struct Docs {
    pub version: String,
    pub origin: Origin,
    pages: Vec<Page>,
    lines: Vec<Line>,
}

/// Where the docs came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The archive of the version's release.
    Release,
    /// waos.app's, the latest release's, which may not be the version's.
    Latest,
}

/// A line of the map: a heading over the lines after it, or a page, each at its depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Heading { title: String, depth: usize },
    Page { name: String, depth: usize },
}

/// A line of a page that matched a search: the page's name, the line's number in the page as
/// it prints, and the line.
#[derive(Debug, PartialEq, Eq)]
pub struct Hit {
    pub page: String,
    pub line: usize,
    pub text: String,
}

/// The docs of the version this directory's app builds with, or this `cargo waos`'s outside an
/// app: from the cache, downloaded into it the first time, or waos.app's latest where that fails,
/// which it says on stderr.
pub async fn load() -> Result<Docs> {
    let version = version::wanted()?;
    let root = cache::root()?;
    if let Some(folder) = cache::bundle(&root, &version) {
        return Docs::from_bundle(&folder);
    }
    match download::archive(&version).await {
        Ok(archive) => {
            let folder = cache::install(&root, &version, &archive)?;
            eprintln!(
                "cargo waos: downloaded the docs of version {version} into {}",
                folder.display()
            );
            Docs::from_bundle(&folder)
        }
        Err(error) => {
            eprintln!(
                "cargo waos: the docs of version {version} could not be downloaded ({error:#}); these are waos.app's latest docs, which may not match version {version}"
            );
            let (pages, playbook) = download::latest().await?;
            Ok(Docs::from_latest(&version, &pages, &playbook))
        }
    }
}

impl Docs {
    /// The docs an unpacked archive in `folder` holds: `manifest.json`, `agents.md` and `docs/`.
    pub fn from_bundle(folder: &Path) -> Result<Docs> {
        bundle::read(folder)
    }

    /// The docs waos.app's `llms-full.txt` and `agents.md` hold, for `version`, which they may not
    /// be.
    pub fn from_latest(version: &str, pages: &str, playbook: &str) -> Docs {
        latest::read(version, pages, playbook)
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
                page.markdown
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
