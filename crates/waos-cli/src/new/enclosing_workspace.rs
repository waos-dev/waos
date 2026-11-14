//! The workspace a new program is made in, found and joined as `cargo new` finds and joins one
//! (cargo's `ops/cargo_new.rs`, `mk`): the nearest `Cargo.toml` above the program's folder, unless
//! its `[workspace.exclude]` names the folder. The program inherits each key of its
//! `[workspace.package]` and its `[workspace.lints]`, as `cargo new` writes them, and each of its
//! dependencies the workspace's `[workspace.dependencies]` lists, as `cargo add` writes one; and
//! the folder is added to `[workspace.members]` unless a pattern there matches it already.
//!
//! The invariant: this does what cargo does and nothing more. A manifest above that does not parse
//! as TOML, or has no `[workspace]`, is left alone and the program made as outside a workspace;
//! the root manifest is edited in place, keeping its comments and layout; and whether the
//! workspace then loads is for cargo to say afterwards (`super::workspace_problem`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{Array, DocumentMut, Item, Value};

/// The files rustup takes a folder's toolchain from.
const TOOLCHAIN_FILES: [&str; 2] = ["rust-toolchain.toml", "rust-toolchain"];

/// What a new program takes from the workspace it is made in. Outside a workspace, nothing.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Inherited {
    /// The keys of `[workspace.package]`, in its order, each written `<key>.workspace = true`.
    pub package_keys: Vec<String>,
    /// Whether the workspace has `[workspace.lints]`, written `[lints] workspace = true`.
    pub lints: bool,
    /// The names `[workspace.dependencies]` lists, each written `<name>.workspace = true`.
    pub dependencies: BTreeSet<String>,
}

/// The workspace a new program's folder is in, with its root manifest as cargo would edit it.
pub struct EnclosingWorkspace {
    root_manifest: PathBuf,
    document: DocumentMut,
    /// The program's folder relative to the workspace's root, joined by `/`, as cargo matches
    /// and writes a member.
    member: String,
}

impl EnclosingWorkspace {
    /// The workspace `folder`, an absolute path, would be a member of: the `[workspace]` of the
    /// nearest `Cargo.toml` above it, unless that excludes `folder` or does not parse.
    pub fn find(folder: &Path) -> Result<Option<EnclosingWorkspace>> {
        let Some(root_manifest) = nearest_manifest_above(folder) else {
            return Ok(None);
        };
        let text = fs::read_to_string(&root_manifest)
            .with_context(|| format!("reading {}", root_manifest.display()))?;
        let Ok(document) = text.parse::<DocumentMut>() else {
            return Ok(None);
        };
        let root = root_manifest.parent().expect("a manifest is in a folder");
        let member = relative_with_slashes(folder, root);
        let found = EnclosingWorkspace {
            root_manifest,
            document,
            member,
        };
        Ok((found.workspace().is_some() && !found.excludes_member()).then_some(found))
    }

    /// The workspace's root folder.
    pub fn root(&self) -> &Path {
        self.root_manifest
            .parent()
            .expect("a manifest is in a folder")
    }

    /// What the program takes from the workspace.
    pub fn inherited(&self) -> Inherited {
        let workspace = self.workspace();
        let table_keys = |name: &str| -> Vec<String> {
            workspace
                .and_then(|workspace| workspace.get(name))
                .and_then(Item::as_table_like)
                .map(|table| table.iter().map(|(key, _)| key.to_owned()).collect())
                .unwrap_or_default()
        };
        Inherited {
            package_keys: table_keys("package"),
            lints: workspace.is_some_and(|workspace| workspace.contains_key("lints")),
            dependencies: table_keys("dependencies").into_iter().collect(),
        }
    }

    /// Whether the workspace's root has a toolchain file of its own, which rustup takes for every
    /// folder beneath it, the program's among them.
    pub fn names_a_toolchain(&self) -> bool {
        TOOLCHAIN_FILES
            .iter()
            .any(|file| self.root().join(file).is_file())
    }

    /// Adds the program's folder to `[workspace.members]` and writes the root manifest, unless a
    /// pattern there matches the folder already; answers whether it added it. Members kept in
    /// order stay in order; a workspace with no members gets them.
    pub fn add_member(&mut self) -> Result<bool> {
        let workspace = self
            .document
            .get_mut("workspace")
            .and_then(Item::as_table_like_mut)
            .expect("found with a [workspace]");
        match workspace.get_mut("members").and_then(Item::as_array_mut) {
            Some(members) => {
                if matches_any(members, &self.member)? {
                    return Ok(false);
                }
                push_keeping_order(members, &self.member);
            }
            None => {
                let members = Array::from_iter([self.member.as_str()]);
                workspace.insert("members", Item::Value(Value::Array(members)));
            }
        }
        fs::write(&self.root_manifest, self.document.to_string())
            .with_context(|| format!("writing {}", self.root_manifest.display()))?;
        Ok(true)
    }

    fn workspace(&self) -> Option<&dyn toml_edit::TableLike> {
        self.document.get("workspace").and_then(Item::as_table_like)
    }

    /// Whether `[workspace.exclude]` names the program's folder, as written.
    fn excludes_member(&self) -> bool {
        self.workspace()
            .and_then(|workspace| workspace.get("exclude"))
            .and_then(Item::as_array)
            .is_some_and(|exclude| {
                exclude
                    .iter()
                    .any(|path| path.as_str() == Some(self.member.as_str()))
            })
    }
}

/// The nearest `Cargo.toml` in a folder above `folder`, as cargo looks for a workspace's root.
fn nearest_manifest_above(folder: &Path) -> Option<PathBuf> {
    folder
        .ancestors()
        .skip(1)
        .map(|ancestor| ancestor.join("Cargo.toml"))
        .find(|manifest| manifest.is_file())
}

/// `folder` relative to `root`, one of its ancestors, with its components joined by `/`.
fn relative_with_slashes(folder: &Path, root: &Path) -> String {
    let relative = folder
        .strip_prefix(root)
        .expect("the root is above the folder");
    let components: Vec<_> = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect();
    components.join("/")
}

/// Whether one of `members`' patterns matches `member`, as cargo's glob matches it.
fn matches_any(members: &Array, member: &str) -> Result<bool> {
    for pattern in members {
        let pattern = pattern
            .as_str()
            .with_context(|| format!("invalid non-string member `{pattern}`"))?;
        let glob = glob::Pattern::new(pattern)
            .with_context(|| format!("cannot build glob pattern from `{pattern}`"))?;
        if glob.matches(member) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Adds `member` to `members`, sorting them again if they were sorted.
fn push_keeping_order(members: &mut Array, member: &str) {
    let was_sorted = members.iter().map(Value::as_str).is_sorted();
    members.push(member);
    if was_sorted {
        members.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A workspace at `root` with `manifest` as its root manifest, and the absolute path of its
    /// folder `member`, made.
    fn workspace(root: &Path, manifest: &str, member: &str) -> PathBuf {
        fs::write(root.join("Cargo.toml"), manifest).unwrap();
        let folder = root.join(member);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn a_folder_with_no_manifest_above_is_in_no_workspace() {
        let scratch = tempfile::tempdir().unwrap();
        assert!(
            EnclosingWorkspace::find(&scratch.path().join("hello"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_package_above_with_no_workspace_is_left_alone() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = workspace(scratch.path(), "[package]\nname = \"outer\"\n", "hello");
        assert!(EnclosingWorkspace::find(&folder).unwrap().is_none());
    }

    #[test]
    fn a_folder_the_workspace_excludes_is_in_no_workspace() {
        let scratch = tempfile::tempdir().unwrap();
        let manifest = "[workspace]\nmembers = [\"apps/*\"]\nexclude = [\"apps/hello\"]\n";
        let folder = workspace(scratch.path(), manifest, "apps/hello");
        assert!(EnclosingWorkspace::find(&folder).unwrap().is_none());
    }

    #[test]
    fn the_program_inherits_the_workspaces_package_keys_lints_and_dependencies() {
        let scratch = tempfile::tempdir().unwrap();
        let manifest = r#"[workspace]
members = ["apps/*"]

[workspace.package]
version = "0.2.0"
edition = "2024"
license = "MIT"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.dependencies]
wasdk = { path = "sdk/wasdk" }
waft = "0.5"
"#;
        let folder = workspace(scratch.path(), manifest, "apps/hello");
        let found = EnclosingWorkspace::find(&folder).unwrap().unwrap();
        assert_eq!(found.root(), scratch.path());
        assert_eq!(
            found.inherited(),
            Inherited {
                package_keys: vec!["version".into(), "edition".into(), "license".into()],
                lints: true,
                dependencies: ["waft".to_owned(), "wasdk".to_owned()].into(),
            }
        );
    }

    #[test]
    fn a_member_a_pattern_matches_leaves_the_root_manifest_as_it_is() {
        let scratch = tempfile::tempdir().unwrap();
        let manifest = "# ours\n[workspace]\nmembers = [\"crates/*\", \"apps/*\"]\n";
        let folder = workspace(scratch.path(), manifest, "apps/hello");
        let mut found = EnclosingWorkspace::find(&folder).unwrap().unwrap();
        assert!(!found.add_member().unwrap());
        let written = fs::read_to_string(scratch.path().join("Cargo.toml")).unwrap();
        assert_eq!(written, manifest);
    }

    #[test]
    fn a_member_no_pattern_matches_is_added_in_order_keeping_the_rest() {
        let scratch = tempfile::tempdir().unwrap();
        let manifest = "# ours\n[workspace]\nmembers = [\"alpha\", \"zulu\"]\n";
        let folder = workspace(scratch.path(), manifest, "hello");
        let mut found = EnclosingWorkspace::find(&folder).unwrap().unwrap();
        assert!(found.add_member().unwrap());
        let written = fs::read_to_string(scratch.path().join("Cargo.toml")).unwrap();
        assert_eq!(
            written,
            "# ours\n[workspace]\nmembers = [\"alpha\", \"hello\", \"zulu\"]\n"
        );
    }

    #[test]
    fn a_member_is_added_last_to_members_out_of_order() {
        let scratch = tempfile::tempdir().unwrap();
        let manifest = "[workspace]\nmembers = [\"zulu\", \"alpha\"]\n";
        let folder = workspace(scratch.path(), manifest, "apps/hello");
        let mut found = EnclosingWorkspace::find(&folder).unwrap().unwrap();
        assert!(found.add_member().unwrap());
        let written: toml::Table = fs::read_to_string(scratch.path().join("Cargo.toml"))
            .unwrap()
            .parse()
            .unwrap();
        let members = written["workspace"]["members"].as_array().unwrap();
        assert_eq!(members.last().unwrap().as_str(), Some("apps/hello"));
    }

    #[test]
    fn a_workspace_with_no_members_gets_the_program_as_its_first() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = workspace(scratch.path(), "[workspace]\n", "hello");
        let mut found = EnclosingWorkspace::find(&folder).unwrap().unwrap();
        assert!(found.add_member().unwrap());
        let written = fs::read_to_string(scratch.path().join("Cargo.toml")).unwrap();
        assert_eq!(written, "[workspace]\nmembers = [\"hello\"]\n");
    }

    #[test]
    fn a_toolchain_file_at_the_root_is_the_workspaces() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = workspace(scratch.path(), "[workspace]\n", "hello");
        let found = EnclosingWorkspace::find(&folder).unwrap().unwrap();
        assert!(!found.names_a_toolchain());
        fs::write(scratch.path().join("rust-toolchain.toml"), "").unwrap();
        assert!(found.names_a_toolchain());
    }
}
