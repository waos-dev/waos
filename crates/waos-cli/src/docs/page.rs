//! One page of the docs, as `cargo waos docs <page>` prints it: its title, its description and
//! its body, in Markdown an agent reads in a terminal.
//!
//! The invariant: a page prints what the site shows, in Markdown. The site's own components are
//! written out as the Markdown they stand for (the icons page's `<AppIcon>`s as a list of links
//! to their pictures on the site), so nothing of MDX reaches the agent.

use super::front_matter::FrontMatter;

/// Where the site serves what a page names by a path, such as an icon's picture.
pub const SITE: &str = "https://waos.app";

/// A page: the name `cargo waos docs` takes, its title and description, and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    pub body: String,
}

impl Page {
    /// The page `text` holds, a site page with its front matter, named `name`.
    pub fn from_site_page(name: &str, text: &str) -> Page {
        let read = FrontMatter::read(text);
        Page {
            name: name.to_owned(),
            title: read.get("title").unwrap_or(name).to_owned(),
            description: read.get("description").map(str::to_owned),
            body: plain_markdown(read.body),
        }
    }

    /// The playbook, `content/agents.md`, which is Markdown with its title as its first heading.
    pub fn from_playbook(name: &str, description: &str, text: &str) -> Page {
        let (title, body) = match text.split_once('\n') {
            Some((first, rest)) if first.starts_with("# ") => (&first[2..], rest),
            _ => (name, text),
        };
        Page {
            name: name.to_owned(),
            title: title.trim().to_owned(),
            description: Some(description.to_owned()),
            body: body.trim_start_matches('\n').to_owned(),
        }
    }

    /// The page as it prints: `# Title`, its description, then its body.
    pub fn markdown(&self) -> String {
        let mut text = format!("# {}\n\n", self.title);
        if let Some(description) = &self.description {
            text.push_str(description);
            text.push_str("\n\n");
        }
        text.push_str(self.body.trim_start_matches('\n'));
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text
    }
}

/// `body` with the site's components written out as Markdown.
fn plain_markdown(body: &str) -> String {
    let mut text = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed == "<AppIcons>" || trimmed == "</AppIcons>" {
            continue;
        }
        match app_icon(trimmed) {
            Some(item) => text.push_str(&item),
            None => text.push_str(line),
        }
        text.push('\n');
    }
    text
}

/// `<AppIcon name="Camera" src="/apps/camera.png" />` as `- Camera: https://waos.app/apps/camera.png`.
fn app_icon(line: &str) -> Option<String> {
    let attributes = line.strip_prefix("<AppIcon ")?.strip_suffix("/>")?;
    let name = attribute(attributes, "name")?;
    let src = attribute(attributes, "src")?;
    let picture = if src.starts_with('/') {
        format!("{SITE}{src}")
    } else {
        src.to_owned()
    };
    Some(format!("- {name}: {picture}"))
}

fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let start = attributes.find(&format!("{name}=\""))? + name.len() + 2;
    let length = attributes[start..].find('"')?;
    Some(&attributes[start..start + length])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_page_prints_its_title_description_and_body() {
        let page = Page::from_site_page(
            "guide/network",
            "---\ntitle: Network\ndescription: Reach the network.\n---\n\nThe network as WASI gives it.\n",
        );
        assert_eq!(
            page.markdown(),
            "# Network\n\nReach the network.\n\nThe network as WASI gives it.\n"
        );
    }

    #[test]
    fn the_icons_are_a_list_of_links_to_their_pictures() {
        let page = Page::from_site_page(
            "icons",
            "---\ntitle: Icons\n---\n\n<AppIcons>\n  <AppIcon name=\"Camera\" src=\"/apps/camera.png\" />\n</AppIcons>\n",
        );
        assert_eq!(
            page.markdown(),
            "# Icons\n\n- Camera: https://waos.app/apps/camera.png\n"
        );
    }

    #[test]
    fn the_playbook_takes_its_title_from_its_heading() {
        let page = Page::from_playbook("playbook", "The steps.", "# Build an app\n\n1. Tools.\n");
        assert_eq!(page.title, "Build an app");
        assert_eq!(
            page.markdown(),
            "# Build an app\n\nThe steps.\n\n1. Tools.\n"
        );
    }
}
