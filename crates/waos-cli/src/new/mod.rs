//! `waos new`: a new program in a folder of its name, a Waft app over
//! `wasdk-adapter-waft` in one widget kit's look, as `cargo waft new` writes a Waft app:
//!
//! ```text
//! hello/
//!   Cargo.toml            the SDK, Waft and the kit, and [package.metadata.waos]
//!   rust-toolchain.toml   WaOS's toolchain, with wasm32-wasip3
//!   icon.png              1024 pixels on a side, which a package carries
//!   src/main.rs           the app
//!   .gitignore
//! ```
//!
//! Made inside a cargo workspace, it joins it as `cargo new` joins one (`enclosing_workspace`):
//! it inherits what the workspace gives, is added to its members unless a pattern there takes it
//! in, and leaves the toolchain to the workspace when the workspace names one.
//!
//! The invariant: a new program is run by `waos run` and packaged by `waos package`
//! as it is written, with no edits: its metadata holds every field `package` reads, its icon
//! passes `waos-package`'s check, and its toolchain is WaOS's own; and it never makes a
//! `[workspace]` of its own, which inside another workspace stops that workspace from loading.
//! `tests/new.rs` builds each template as a component.

mod enclosing_workspace;
pub(crate) mod icon;
mod manifest;
mod program_name;
mod template;

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use cargo_metadata::MetadataCommand;

use self::enclosing_workspace::EnclosingWorkspace;
use self::program_name::ProgramName;
pub use self::template::Template;

/// The toolchain a program is built with: WaOS's own, the nightly Waft needs at its pinned date,
/// with `wasm32-wasip3`, so rustup installs both by itself.
const TOOLCHAIN: &str = r#"# Nightly because Waft needs `arbitrary_self_types`, pinned to the date WaOS is built with.
# `wasm32-wasip3` is what a program is built for.
[toolchain]
channel = "nightly-2026-11-15"
components = ["rustfmt", "clippy"]
targets = ["wasm32-wasip3"]
"#;

/// What git leaves out: cargo's build.
const GITIGNORE: &str = "/target\n";

/// A program `create` wrote.
#[derive(Debug)]
pub struct NewProgram {
    pub folder: PathBuf,
    /// The root of the workspace whose members it was added to, if it was.
    pub added_to: Option<PathBuf>,
}

/// Writes a new program named `name`, with `template`'s kit, in a folder of that name in
/// `parent`, joining the workspace `parent` is in. A name that is not a package name, or a folder
/// there already, is refused before anything is written.
pub fn create(name: &str, template: Template, parent: &Path) -> Result<NewProgram> {
    let name: ProgramName = name.parse()?;
    let folder = parent.join(name.as_str());
    let absolute = std::path::absolute(&folder)
        .with_context(|| format!("finding where {} is", folder.display()))?;
    make_folder(&folder)?;
    let mut workspace = EnclosingWorkspace::find(&absolute)?;
    write_program(&folder, &name, template, workspace.as_ref())?;
    let added_to = match &mut workspace {
        Some(workspace) => workspace
            .add_member()?
            .then(|| workspace.root().to_path_buf()),
        None => None,
    };
    Ok(NewProgram { folder, added_to })
}

/// Why cargo cannot load the workspace the program at `folder` is in, if it cannot, as `cargo new`
/// checks it once the package is written.
pub fn workspace_problem(folder: &Path) -> Option<anyhow::Error> {
    MetadataCommand::new()
        .manifest_path(folder.join("Cargo.toml"))
        .no_deps()
        .exec()
        .err()
        .map(anyhow::Error::from)
}

/// Makes `folder`, which must not be there yet.
fn make_folder(folder: &Path) -> Result<()> {
    match fs::create_dir(folder) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            bail!("{} already exists", folder.display())
        }
        Err(error) => Err(error).with_context(|| format!("making {}", folder.display())),
    }
}

/// Writes the program's files into `folder`, a member of `workspace` if it is in one.
fn write_program(
    folder: &Path,
    name: &ProgramName,
    template: Template,
    workspace: Option<&EnclosingWorkspace>,
) -> Result<()> {
    let inherited = workspace
        .map(EnclosingWorkspace::inherited)
        .unwrap_or_default();
    write(
        folder,
        "Cargo.toml",
        manifest::cargo_toml(name, template, &inherited),
    )?;
    if !workspace.is_some_and(EnclosingWorkspace::names_a_toolchain) {
        write(folder, "rust-toolchain.toml", TOOLCHAIN)?;
    }
    write(folder, icon::FILE, icon::placeholder())?;
    write(folder, ".gitignore", GITIGNORE)?;
    let source = folder.join("src");
    fs::create_dir(&source).with_context(|| format!("making {}", source.display()))?;
    write(folder, "src/main.rs", template.main_rs())
}

/// Writes `contents` to the file `file` names in `folder`.
fn write(folder: &Path, file: &str, contents: impl AsRef<[u8]>) -> Result<()> {
    let path = folder.join(file);
    fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use cargo_metadata::MetadataCommand;
    use waos_package::{Manifest, SigningKey};

    use super::*;
    use crate::metadata::WaosMetadata;

    #[test]
    fn a_new_program_has_its_manifest_toolchain_icon_ignores_and_main() {
        let scratch = tempfile::tempdir().unwrap();
        let created = create("hello", Template::Winui, scratch.path()).unwrap();
        let folder = created.folder;
        assert_eq!(folder, scratch.path().join("hello"));
        assert_eq!(created.added_to, None);
        let mut written: Vec<String> = walk(&folder)
            .iter()
            .map(|path| relative_with_slashes(path.strip_prefix(&folder).unwrap()))
            .collect();
        written.sort();
        assert_eq!(
            written,
            [
                ".gitignore",
                "Cargo.toml",
                "icon.png",
                "rust-toolchain.toml",
                "src/main.rs"
            ]
        );
        let main_rs = fs::read_to_string(folder.join("src/main.rs")).unwrap();
        assert_eq!(main_rs, Template::Winui.main_rs());
    }

    #[test]
    fn each_template_runs_a_waft_app_in_its_kit() {
        for (template, app) in [
            (Template::Winui, "FluentApp"),
            (Template::Cupertino, "CupertinoApp"),
        ] {
            let main_rs = template.main_rs();
            // Line by line, as a checkout on Windows writes them with `\r\n`.
            let lines: Vec<&str> = main_rs.lines().collect();
            assert!(
                lines
                    .windows(2)
                    .any(|pair| pair[0] == "#[wasdk::main]"
                        && pair[1].starts_with("async fn main()")),
                "{template:?}"
            );
            assert!(main_rs.contains("wasdk_adapter_waft::run("));
            assert!(main_rs.contains(app), "{template:?}");
            assert!(main_rs.contains(&template.kit().replace('-', "_")));
        }
    }

    #[test]
    fn a_folder_there_already_is_left_as_it_is() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path().join("hello");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("notes.txt"), "mine").unwrap();
        let error = create("hello", Template::Cupertino, scratch.path()).unwrap_err();
        assert!(error.to_string().contains("already exists"), "{error}");
        assert_eq!(walk(&folder), [folder.join("notes.txt")]);
    }

    #[test]
    fn a_name_that_is_not_a_package_name_writes_nothing() {
        let scratch = tempfile::tempdir().unwrap();
        let error = create("My App", Template::Cupertino, scratch.path()).unwrap_err();
        assert!(error.to_string().contains("not a package name"), "{error}");
        assert!(walk(scratch.path()).is_empty());
    }

    /// What `waos package` reads of a new program, through cargo as it reads it, makes a
    /// package: its metadata and version give a manifest, and its icon passes the check.
    #[test]
    fn a_new_program_has_what_a_package_needs() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = create("my-app", Template::Cupertino, scratch.path())
            .unwrap()
            .folder;
        let metadata = MetadataCommand::new()
            .manifest_path(folder.join("Cargo.toml"))
            .no_deps()
            .exec()
            .unwrap();
        let package = metadata.root_package().unwrap();
        let waos = WaosMetadata::of(package).unwrap();
        assert_eq!(waos.id.as_str(), "dev.example.myapp");
        let manifest = Manifest::new(waos.id, waos.name, package.version.clone()).unwrap();
        let component = scratch.path().join("program.wasm");
        fs::write(&component, b"\0asm").unwrap();
        let written = scratch.path().join("my-app.wapk");
        waos_package::write(
            &written,
            &manifest,
            &component,
            &waos.icon,
            &SigningKey::generate(),
        )
        .unwrap();
    }

    #[test]
    fn the_toolchain_is_waos_own() {
        let waos = concat!(env!("CARGO_MANIFEST_DIR"), "/../../rust-toolchain.toml");
        let waos: toml::Table = fs::read_to_string(waos).unwrap().parse().unwrap();
        let program: toml::Table = TOOLCHAIN.parse().unwrap();
        assert_eq!(program, waos);
    }

    /// A workspace in `root` whose members are `members`, written as TOML, which gives package
    /// keys, lints and two dependencies, with a toolchain of its own when `toolchain` says so, and
    /// an empty `apps` folder.
    fn workspace_with(root: &Path, members: &str, toolchain: bool) {
        let head = format!("[workspace]\nmembers = {members}\n");
        let manifest = head
            + r#"
[workspace.package]
version = "0.2.0"
edition = "2024"
license = "MIT"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.dependencies]
wasdk = "0.2"
waft = "0.6"
"#;
        fs::write(root.join("Cargo.toml"), manifest).unwrap();
        fs::create_dir(root.join("apps")).unwrap();
        if toolchain {
            fs::write(root.join("rust-toolchain.toml"), TOOLCHAIN).unwrap();
        }
    }

    /// As `cargo new` makes a package in a folder its workspace's members take in: a member that
    /// inherits, no `[workspace]` of its own, the root manifest untouched, the workspace loading
    /// with it among its members, and the toolchain left to the workspace.
    #[test]
    fn a_program_made_where_a_workspace_takes_it_in_is_its_member() {
        let scratch = tempfile::tempdir().unwrap();
        workspace_with(scratch.path(), r#"["apps/*"]"#, true);
        let root_manifest = fs::read_to_string(scratch.path().join("Cargo.toml")).unwrap();
        let created = create("hello", Template::Winui, &scratch.path().join("apps")).unwrap();
        assert_eq!(created.added_to, None);
        assert_eq!(
            fs::read_to_string(scratch.path().join("Cargo.toml")).unwrap(),
            root_manifest
        );
        let manifest: toml::Table = fs::read_to_string(created.folder.join("Cargo.toml"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(manifest.get("workspace").is_none());
        assert_eq!(
            manifest["package"]["license"]["workspace"].as_bool(),
            Some(true)
        );
        assert_eq!(manifest["lints"]["workspace"].as_bool(), Some(true));
        assert_eq!(
            manifest["dependencies"]["wasdk"]["workspace"].as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["dependencies"]["waft-winui"].as_str(),
            Some("0.10")
        );
        assert!(!created.folder.join("rust-toolchain.toml").exists());
        assert!(workspace_problem(&created.folder).is_none());
        let metadata = MetadataCommand::new()
            .manifest_path(scratch.path().join("Cargo.toml"))
            .no_deps()
            .exec()
            .unwrap();
        assert!(
            metadata
                .workspace_packages()
                .iter()
                .any(|package| package.name.as_str() == "hello")
        );
    }

    #[test]
    fn a_program_no_member_pattern_takes_in_is_added_to_the_members() {
        let scratch = tempfile::tempdir().unwrap();
        workspace_with(scratch.path(), r#"["apps"]"#, false);
        fs::write(
            scratch.path().join("apps/Cargo.toml"),
            "[package]\nname = \"apps\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::create_dir(scratch.path().join("apps/src")).unwrap();
        fs::write(scratch.path().join("apps/src/lib.rs"), "").unwrap();
        let created = create("hello", Template::Cupertino, scratch.path()).unwrap();
        assert_eq!(created.added_to.as_deref(), Some(scratch.path()));
        let root: toml::Table = fs::read_to_string(scratch.path().join("Cargo.toml"))
            .unwrap()
            .parse()
            .unwrap();
        let members: Vec<_> = root["workspace"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|member| member.as_str().unwrap())
            .collect();
        assert_eq!(members, ["apps", "hello"]);
        assert!(created.folder.join("rust-toolchain.toml").exists());
        assert!(workspace_problem(&created.folder).is_none());
    }

    #[test]
    fn a_program_its_workspace_excludes_takes_nothing_from_it() {
        let scratch = tempfile::tempdir().unwrap();
        let manifest = "[workspace]\nmembers = [\"apps/*\"]\nexclude = [\"apps/hello\"]\n\n[workspace.package]\nlicense = \"MIT\"\n";
        fs::write(scratch.path().join("Cargo.toml"), manifest).unwrap();
        fs::create_dir(scratch.path().join("apps")).unwrap();
        let created = create("hello", Template::Winui, &scratch.path().join("apps")).unwrap();
        assert_eq!(created.added_to, None);
        let written = fs::read_to_string(created.folder.join("Cargo.toml")).unwrap();
        assert!(!written.contains("workspace"), "{written}");
        assert!(workspace_problem(&created.folder).is_none());
    }

    /// As `cargo new` warns when the package it wrote leaves its workspace unable to load.
    #[test]
    fn a_workspace_that_cannot_load_with_the_program_is_said() {
        let scratch = tempfile::tempdir().unwrap();
        workspace_with(scratch.path(), r#"["apps/*"]"#, true);
        let other = scratch.path().join("apps/other");
        fs::create_dir_all(other.join("src")).unwrap();
        fs::write(
            other.join("Cargo.toml"),
            "[package]\nname = \"hello\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(other.join("src/main.rs"), "fn main() {}\n").unwrap();
        let created = create("hello", Template::Winui, &scratch.path().join("apps")).unwrap();
        let problem = workspace_problem(&created.folder).expect("two packages named hello");
        assert!(format!("{problem:#}").contains("hello"), "{problem:#}");
    }

    /// Every file under `folder`, at any depth.
    fn walk(folder: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(walk(&path));
            } else {
                files.push(path);
            }
        }
        files
    }

    /// `path` with its components joined by `/`, as the test names them on every system.
    fn relative_with_slashes(path: &Path) -> String {
        let components: Vec<_> = path
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect();
        components.join("/")
    }
}
