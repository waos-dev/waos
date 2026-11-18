//! Builds the docs into `cargo waos`: writes `$OUT_DIR/content.rs`, every file under `content/`
//! (the site's docs, which `cargo xtask sync-docs` copies there) by its path from that folder,
//! each held as `include_str!` holds it, in the order of their paths.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("content");
    println!("cargo::rerun-if-changed={}", content.display());
    let mut files = Vec::new();
    add_files(&content, Path::new(""), &mut files)?;
    files.sort();
    let entries: String = files
        .iter()
        .map(|file| {
            let name = written_with_slashes(file);
            let path = content.join(file);
            format!(
                "    ({name:?}, include_str!({:?})),\n",
                path.display().to_string()
            )
        })
        .collect();
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    fs::write(out.join("content.rs"), format!("&[\n{entries}]\n"))
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
