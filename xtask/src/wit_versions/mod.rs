//! The WIT's versions against the last release's: a program built against a release imports that
//! release's packages by name and version, so a package whose contents change keeps those programs
//! running only under a new version. `cargo xtask check-wit-versions` compares `wit/` with the last
//! `v*` tag's and names every package that changed under the same version.
//!
//! The invariant a later editor will break: a package's contract is its files' text without
//! comments or the spacing between words, so a doc comment or reflowed line changes nothing and
//! every other edit, an added function as much as a removed one, needs a new version.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

/// A `.wit` file, by its path from the workspace's root, and its text.
pub struct WitFile {
    pub path: String,
    pub text: String,
}

/// Each package's contract, by its `name@version`: its files' text in the order of their paths,
/// without comments, every run of spacing one space.
pub fn contracts(files: &[WitFile]) -> BTreeMap<String, String> {
    let packages_by_folder: BTreeMap<&str, String> = files
        .iter()
        .filter_map(|file| Some((folder_of(&file.path), declared_package(&file.text)?)))
        .collect();
    let mut sorted: Vec<&WitFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut contracts: BTreeMap<String, String> = BTreeMap::new();
    for file in sorted {
        let Some(package) = packages_by_folder.get(folder_of(&file.path)) else {
            continue;
        };
        let contract = contracts.entry(package.clone()).or_default();
        contract.push_str(&without_comments(&file.text));
        contract.push('\n');
    }
    contracts
}

/// The packages present in both whose contract differs, each by its `name@version`.
pub fn changed_under_same_version(
    released: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Vec<String> {
    current
        .iter()
        .filter(|(package, contract)| released.get(*package).is_some_and(|old| old != *contract))
        .map(|(package, _)| package.clone())
        .collect()
}

/// The last release's tag and every package changed since it under the same version; none before
/// the first release.
pub fn compare_with_last_release(root: &Path) -> io::Result<Option<(String, Vec<String>)>> {
    let Some(tag) = last_release(root)? else {
        return Ok(None);
    };
    let released = contracts(&released_files(root, &tag)?);
    let current = contracts(&current_files(root)?);
    let changed = changed_under_same_version(&released, &current);
    Ok(Some((tag, changed)))
}

/// The last release's tag, the nearest `v*` tag behind `HEAD`; none before the first release.
pub fn last_release(root: &Path) -> io::Result<Option<String>> {
    let output = git(root, &["describe", "--tags", "--abbrev=0", "--match", "v*"])?;
    Ok(output
        .status
        .success()
        .then(|| stdout(&output).trim().to_owned()))
}

/// The `.wit` files under `wit/` as `tag` has them.
pub fn released_files(root: &Path, tag: &str) -> io::Result<Vec<WitFile>> {
    let listed = git(root, &["ls-tree", "-r", "--name-only", tag, "--", "wit"])?;
    let mut files = Vec::new();
    for path in stdout(&listed)
        .lines()
        .filter(|path| path.ends_with(".wit"))
    {
        let shown = git(root, &["show", &format!("{tag}:{path}")])?;
        files.push(WitFile {
            path: path.to_owned(),
            text: stdout(&shown),
        });
    }
    Ok(files)
}

/// The `.wit` files under `wit/` as the checkout has them.
pub fn current_files(root: &Path) -> io::Result<Vec<WitFile>> {
    let mut files = Vec::new();
    collect_wit_files(root, Path::new("wit"), &mut files)?;
    Ok(files)
}

fn collect_wit_files(root: &Path, folder: &Path, files: &mut Vec<WitFile>) -> io::Result<()> {
    for entry in fs::read_dir(root.join(folder))? {
        let entry = entry?;
        let path = folder.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            collect_wit_files(root, &path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "wit") {
            files.push(WitFile {
                path: path.to_string_lossy().replace('\\', "/"),
                text: fs::read_to_string(root.join(&path))?,
            });
        }
    }
    Ok(())
}

/// The `name@version` a file's `package` declaration names, if it has one.
fn declared_package(text: &str) -> Option<String> {
    let code = without_comments(text);
    let rest = code.strip_prefix("package ")?;
    Some(rest.split(';').next()?.trim().to_owned())
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// `text` without `//` and `/* */` comments, every run of spacing one space.
fn without_comments(text: &str) -> String {
    let mut code = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("//") {
            rest = after.split_once('\n').map_or("", |(_, next)| next);
            code.push(' ');
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after.split_once("*/").map_or("", |(_, next)| next);
            code.push(' ');
        } else {
            let character = rest.chars().next().expect("rest is not empty");
            code.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    code.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn git(root: &Path, arguments: &[&str]) -> io::Result<std::process::Output> {
    Command::new("git")
        .current_dir(root)
        .args(arguments)
        .output()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
