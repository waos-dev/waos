//! A page of the docs, as `cargo waos docs <page>` prints it.

/// The site's origin, which its addresses and the icons' pictures are at.
pub const SITE: &str = "https://waos.app";

/// A page: its name, its title and one line, as the map shows them, and its Markdown, its title
/// as its first heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    pub markdown: String,
}

impl Page {
    /// The playbook, Markdown whose first line is its title as a heading.
    pub fn playbook(name: &str, description: &str, markdown: &str) -> Page {
        let title = markdown
            .lines()
            .next()
            .and_then(|first| first.strip_prefix("# "))
            .unwrap_or(name);
        Page {
            name: name.to_owned(),
            title: title.trim().to_owned(),
            description: Some(description.to_owned()),
            markdown: ended(markdown),
        }
    }

    /// A page as the playbook's Markdown is, without its title's heading.
    pub fn body(&self) -> &str {
        let heading = format!("# {}\n", self.title);
        self.markdown
            .strip_prefix(&heading)
            .unwrap_or(&self.markdown)
            .trim_start_matches('\n')
    }
}

/// `text` ending with one line break.
pub fn ended(text: &str) -> String {
    format!("{}\n", text.trim_end())
}
