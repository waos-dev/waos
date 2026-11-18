//! The docs an unpacked release archive holds: `manifest.json`, the version and the pages in the
//! map's order, each with its title and one line; `agents.md`, the playbook; and `docs/<page>.md`,
//! each page as plain Markdown. The site's `write-docs-bundle.mjs` writes them.
//!
//! The invariant: every page the manifest lists is a file, and only those are pages: a file it
//! does not list is not reached.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::page::{Page, ended};
use super::{Docs, Line, Origin, PLAYBOOK, PLAYBOOK_DESCRIPTION};

/// `manifest.json`.
#[derive(Deserialize)]
struct Manifest {
    version: String,
    pages: Vec<Entry>,
}

/// A line of the manifest's map.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Entry {
    Heading {
        title: String,
        depth: usize,
    },
    Page {
        name: String,
        title: String,
        description: Option<String>,
        depth: usize,
    },
}

/// The docs in `folder`.
pub fn read(folder: &Path) -> Result<Docs> {
    let manifest: Manifest = serde_json::from_str(&read_text(&folder.join("manifest.json"))?)
        .with_context(|| format!("reading {}", folder.join("manifest.json").display()))?;
    let playbook = read_text(&folder.join("agents.md"))?;
    let mut pages = vec![Page::playbook(PLAYBOOK, PLAYBOOK_DESCRIPTION, &playbook)];
    let mut lines = vec![Line::Page {
        name: PLAYBOOK.to_owned(),
        depth: 0,
    }];
    for entry in manifest.pages {
        match entry {
            Entry::Heading { title, depth } => lines.push(Line::Heading { title, depth }),
            Entry::Page {
                name,
                title,
                description,
                depth,
            } => {
                let markdown = read_text(&folder.join("docs").join(format!("{name}.md")))?;
                lines.push(Line::Page {
                    name: name.clone(),
                    depth,
                });
                pages.push(Page {
                    name,
                    title,
                    description,
                    markdown: ended(&markdown),
                });
            }
        }
    }
    Ok(Docs {
        version: manifest.version,
        origin: Origin::Release,
        pages,
        lines,
    })
}

fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}
