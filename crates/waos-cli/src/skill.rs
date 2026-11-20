//! The skill `cargo waos docs skill install` writes: a folder an agent loads when it builds a WaOS
//! app, holding `SKILL.md`, the playbook under the front matter that says when to use it, and
//! `docs/`, every page of this version's docs as `cargo waos docs <page>` prints it, with
//! `docs/index.md` the map.
//!
//! The invariant: an install leaves the skill as this version has it. `SKILL.md` and `docs/`
//! are replaced whole, so a page this version no longer has goes; nothing else in the folder is
//! touched.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::docs::{self, Docs};

/// The skill's folder's name, and its `name` in `SKILL.md`.
pub const NAME: &str = "waos";

/// When an agent uses the skill, as `SKILL.md`'s front matter says it.
const DESCRIPTION: &str = "Build, run, try and package an app for WaOS with cargo waos, Waft and wasdk. Use when making a WaOS app or program, writing its UI with Waft, using a platform capability through wasdk, or running, screenshotting, giving input to or packaging it with cargo waos.";

/// The skills folder an install writes into without `--dir`: Claude Code's personal one,
/// `~/.claude/skills`, which every project of the user's sees, as every app they make wants it.
pub fn default_skills_folder() -> Result<PathBuf> {
    let home = std::env::home_dir().context("no home folder to find ~/.claude/skills in")?;
    Ok(home.join(".claude").join("skills"))
}

/// Writes the skill as `docs` has it into `skills/waos`, and answers that folder.
pub fn install(skills: &Path, docs: &Docs) -> Result<PathBuf> {
    let folder = skills.join(NAME);
    write_files(&folder, docs)
        .with_context(|| format!("writing the skill at {}", folder.display()))?;
    Ok(folder)
}

fn write_files(folder: &Path, docs: &Docs) -> io::Result<()> {
    let pages = folder.join("docs");
    remove_folder_if_there(&pages)?;
    fs::create_dir_all(&pages)?;
    fs::write(folder.join("SKILL.md"), skill_md(docs))?;
    fs::write(pages.join("index.md"), docs::skill_index(docs))?;
    for page in docs
        .pages()
        .iter()
        .filter(|page| page.name != docs::PLAYBOOK)
    {
        let file = pages.join(format!("{}.md", page.name));
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file, &page.markdown)?;
    }
    Ok(())
}

/// `SKILL.md`: its front matter, where this version's docs are, then the playbook.
fn skill_md(docs: &Docs) -> String {
    let playbook = docs
        .find(docs::PLAYBOOK)
        .map(|page| format!("# {}\n\n{}", page.title, page.body()))
        .unwrap_or_default();
    format!(
        "---\nname: {NAME}\ndescription: {DESCRIPTION}\n---\n\n{} are in `docs/`, a page a file; `docs/index.md` is their map.\n\n{playbook}",
        docs::whose(docs)
    )
}

fn remove_folder_if_there(folder: &Path) -> io::Result<()> {
    match fs::remove_dir_all(folder) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Docs of a few pages, as waos.app's `llms-full.txt` and `agents.md` have them.
    fn docs() -> Docs {
        Docs::from_latest(
            "9.8.7",
            "# Start (https://waos.app/docs/start)\n\nInstall.\n\n# Building the app (https://waos.app/docs/guide)\n\n# Network (https://waos.app/docs/guide/network)\n\nHTTP.\n",
            "# Build an app on WaOS\n\n1. **Tools:** rustup.\n",
        )
    }

    #[test]
    fn the_skill_is_the_playbook_and_every_page_as_a_file() {
        let skills = tempfile::tempdir().unwrap();
        let docs = docs();
        let folder = install(skills.path(), &docs).unwrap();
        assert_eq!(folder, skills.path().join("waos"));
        let skill = fs::read_to_string(folder.join("SKILL.md")).unwrap();
        assert!(
            skill.starts_with("---\nname: waos\ndescription: Build, run"),
            "{skill}"
        );
        assert!(skill.contains("`docs/index.md` is their map"), "{skill}");
        assert!(
            skill.ends_with("# Build an app on WaOS\n\n1. **Tools:** rustup.\n"),
            "{skill}"
        );
        assert_eq!(
            fs::read_to_string(folder.join("docs/guide/network.md")).unwrap(),
            "# Network\n\nHTTP.\n"
        );
        assert!(folder.join("docs/guide.md").exists());
        assert!(folder.join("docs/start.md").exists());
        assert!(!folder.join("docs/playbook.md").exists());
        let index = fs::read_to_string(folder.join("docs/index.md")).unwrap();
        assert!(index.contains("[Network](guide/network.md)"), "{index}");
    }

    #[test]
    fn an_install_replaces_the_docs_whole_and_keeps_the_rest_of_the_folder() {
        let skills = tempfile::tempdir().unwrap();
        let folder = skills.path().join("waos");
        fs::create_dir_all(folder.join("docs/gone")).unwrap();
        fs::write(folder.join("docs/gone/page.md"), "old").unwrap();
        fs::write(folder.join("notes.md"), "the user's").unwrap();
        install(skills.path(), &docs()).unwrap();
        assert!(!folder.join("docs/gone").exists());
        assert_eq!(
            fs::read_to_string(folder.join("notes.md")).unwrap(),
            "the user's"
        );
    }
}
