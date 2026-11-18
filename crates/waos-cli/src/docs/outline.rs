//! The docs' order, as the site's sidebar has it: each folder's `meta.json` lists its pages and
//! folders in order, a `---Name---` among them heading the ones after it, and a folder's
//! `index` page is the folder's own line. Read from the files under `content/docs`.
//!
//! The invariant: every page under `content/docs` has one line. A page or folder a `meta.json`
//! leaves out comes after the folder's listed ones, by name, where the site would hide it, so no
//! page is out of reach of `cargo waos docs`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::page::Page;

/// A line of the outline: a heading over the lines after it, or a page, each at its depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Heading { title: String, depth: usize },
    Page { name: String, depth: usize },
}

/// What a folder's `meta.json` lists, in order.
enum Listed {
    Separator(String),
    Item(String),
}

/// The pages under `docs/` of `files`, each a site page by its path from the content folder, in
/// the outline's order, and the outline's lines, starting at `depth`.
pub fn read(files: &BTreeMap<&str, &str>, depth: usize) -> (Vec<Page>, Vec<Line>) {
    let mut outline = Outline {
        files,
        pages: Vec::new(),
        lines: Vec::new(),
    };
    outline.add_folder("docs", depth);
    (outline.pages, outline.lines)
}

struct Outline<'a> {
    files: &'a BTreeMap<&'a str, &'a str>,
    pages: Vec<Page>,
    lines: Vec<Line>,
}

impl<'a> Outline<'a> {
    /// Adds the pages and folders in `folder`, in its `meta.json`'s order, at `depth`.
    fn add_folder(&mut self, folder: &str, depth: usize) {
        let mut item_depth = depth;
        for listed in self.order(folder) {
            match listed {
                Listed::Separator(title) => {
                    self.lines.push(Line::Heading { title, depth });
                    item_depth = depth + 1;
                }
                Listed::Item(name) => self.add_item(&format!("{folder}/{name}"), item_depth),
            }
        }
    }

    /// Adds the page or folder at `path`: a folder as its index page, or a heading of its title
    /// where it has none, then what it holds.
    fn add_item(&mut self, path: &str, depth: usize) {
        if let Some(text) = self.page_file(path) {
            self.add_page(path, text, depth);
            return;
        }
        match self.page_file(&format!("{path}/index")) {
            Some(text) => self.add_page(path, text, depth),
            None => {
                let title = self
                    .meta_title(path)
                    .unwrap_or_else(|| name_of(path).to_owned());
                self.lines.push(Line::Heading { title, depth });
            }
        }
        self.add_folder(path, depth + 1);
    }

    fn add_page(&mut self, path: &str, text: &str, depth: usize) {
        let name = path.strip_prefix("docs/").unwrap_or(path).to_owned();
        self.pages.push(Page::from_site_page(&name, text));
        self.lines.push(Line::Page { name, depth });
    }

    /// The folder's items: those its `meta.json` lists that are there, then the rest by name.
    fn order(&self, folder: &str) -> Vec<Listed> {
        let present = self.children(folder);
        let mut order: Vec<Listed> = self
            .meta(folder)
            .and_then(|meta| meta.get("pages").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
            .filter_map(listed)
            .filter(|item| match item {
                Listed::Item(name) => present.contains(name.as_str()),
                Listed::Separator(_) => true,
            })
            .collect();
        let named: BTreeSet<String> = order
            .iter()
            .filter_map(|item| match item {
                Listed::Item(name) => Some(name.clone()),
                Listed::Separator(_) => None,
            })
            .collect();
        order.extend(
            present
                .into_iter()
                .filter(|name| !named.contains(*name))
                .map(|name| Listed::Item(name.to_owned())),
        );
        order
    }

    /// The names of the pages and folders in `folder`, but its index and `meta.json`.
    fn children(&self, folder: &str) -> BTreeSet<&'a str> {
        let files: &'a BTreeMap<&'a str, &'a str> = self.files;
        let prefix = format!("{folder}/");
        files
            .keys()
            .filter_map(|path| path.strip_prefix(prefix.as_str()))
            .filter_map(|rest| match rest.split_once('/') {
                Some((subfolder, _)) => Some(subfolder),
                None => rest
                    .strip_suffix(".mdx")
                    .or_else(|| rest.strip_suffix(".md"))
                    .filter(|stem| *stem != "index"),
            })
            .collect()
    }

    fn page_file(&self, path: &str) -> Option<&'a str> {
        let files: &'a BTreeMap<&'a str, &'a str> = self.files;
        [".mdx", ".md"]
            .iter()
            .find_map(|extension| files.get(format!("{path}{extension}").as_str()))
            .copied()
    }

    fn meta(&self, folder: &str) -> Option<Value> {
        let text = self.files.get(format!("{folder}/meta.json").as_str())?;
        serde_json::from_str(text).ok()
    }

    fn meta_title(&self, folder: &str) -> Option<String> {
        let meta = self.meta(folder)?;
        meta.get("title").and_then(Value::as_str).map(str::to_owned)
    }
}

/// An entry of `meta.json`'s `pages`: `---Name---` a separator, a name an item; the rest (`...`,
/// a link) has no line.
fn listed(entry: &str) -> Option<Listed> {
    if let Some(title) = entry
        .strip_prefix("---")
        .and_then(|e| e.strip_suffix("---"))
    {
        return Some(Listed::Separator(title.to_owned()));
    }
    let is_name = !entry.is_empty() && !entry.contains("...") && !entry.starts_with('[');
    is_name.then(|| Listed::Item(entry.to_owned()))
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
