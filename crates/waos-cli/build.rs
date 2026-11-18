//! Builds the docs into `cargo waos`: writes `$OUT_DIR/docs.rs`, every file of the docs by its
//! path, each held as `include_str!` holds it, in the order of their paths. They are the
//! playbook, `playbook.md`; the pages under `docs/`, which the website renders too; and the
//! reference's page for each interface, made here from the WIT under `wit/`, which
//! `cargo xtask sync-wit` copies there, as the website makes its own at its build.
//!
//! The invariant: the reference's pages for each interface are made, never read from `docs/`,
//! where the website's build writes its own and nothing commits them.

#[path = "build/reference.rs"]
mod reference;

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The folder under `docs/reference/` the website's build writes each package's pages in starts
/// so: `wasdk-platform`.
const GENERATED_REFERENCE: &str = "docs/reference/wasdk-";

fn main() -> io::Result<()> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    for watched in ["docs", "playbook.md", "wit"] {
        println!(
            "cargo::rerun-if-changed={}",
            crate_root.join(watched).display()
        );
    }
    let mut files = written_docs(crate_root)?;
    files.extend(reference::write_pages(
        &crate_root.join("wit"),
        &out.join("reference"),
    )?);
    files.sort();
    fs::write(out.join("docs.rs"), included(&files))
}

/// The playbook and every page under `docs/` but the reference's made ones, each by its path in
/// the docs and where it is.
fn written_docs(crate_root: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut files = vec![("playbook.md".to_owned(), crate_root.join("playbook.md"))];
    let mut pages = Vec::new();
    add_files(&crate_root.join("docs"), Path::new("docs"), &mut pages)?;
    for page in pages {
        let name = written_with_slashes(&page);
        if !name.starts_with(GENERATED_REFERENCE) {
            files.push((name, crate_root.join(page)));
        }
    }
    Ok(files)
}

/// `files` as the Rust of a slice of their names and contents.
fn included(files: &[(String, PathBuf)]) -> String {
    let entries: String = files
        .iter()
        .map(|(name, path)| {
            format!(
                "    ({name:?}, include_str!({:?})),\n",
                path.display().to_string()
            )
        })
        .collect();
    format!("&[\n{entries}]\n")
}

/// Adds every file under `folder` to `files`, by its path from the folder `relative` names.
fn add_files(folder: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            add_files(&entry.path(), &path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn written_with_slashes(path: &Path) -> String {
    let parts: Vec<_> = path.iter().map(|part| part.to_string_lossy()).collect();
    parts.join("/")
}
