//! `waos new`'s programs build as components, each template's, with the binary as a
//! program's author runs it: `waos new` in a folder, WinUI's without `--template`, then
//! `waos build` in the program.
//!
//! A new program names the SDK and Waft by their crates.io versions, where it would build against
//! the last release, so each is built against this workspace's crates instead, to test a change
//! before it is published: a `.cargo/config.toml` in the folder above it patches crates.io with
//! the workspace's path of every crate its manifest names, and it gets the workspace's
//! `Cargo.lock`, so it builds with the versions the workspace is tested with. The written program itself is left as `new` wrote it. The programs are built in the
//! target directory `waos`'s test programs are built in, `<target>/test-programs/target`, with
//! their profile, so the SDK and Waft those built are reused.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The preamble of a component's binary: wasm's magic, then the component encoding's version and
/// layer.
const COMPONENT_PREAMBLE: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];

#[test]
fn a_new_program_is_winui_by_default_and_builds_as_a_component() {
    builds_as_a_component("winui", &[]);
}

#[test]
fn a_new_cupertino_program_builds_as_a_component() {
    builds_as_a_component("cupertino", &["--template", "cupertino"]);
}

/// Writes a program with `new`'s `options`, checks that it is `template`'s, builds it against the
/// workspace, and checks that the component `build` says it made is one.
fn builds_as_a_component(template: &str, options: &[&str]) {
    let scratch = tempfile::tempdir().unwrap();
    let name = format!("new-{template}");
    let said = run_waos(scratch.path(), &[&["new", name.as_str()], options].concat());
    assert!(said.contains(&format!("cd {name} && waos run")), "{said}");
    let program = scratch.path().join(&name);
    let main_rs = fs::read_to_string(program.join("src/main.rs")).unwrap();
    assert_eq!(main_rs, template_main_rs(template), "not {template}'s");
    build_against_the_workspace(scratch.path(), &program);
    let component = run_waos(&program, &["build"]);
    let component = fs::read(component.trim()).unwrap();
    assert_eq!(component[..8], COMPONENT_PREAMBLE, "not a component");
}

/// Runs `waos` with `arguments` in `folder`, and answers what it printed on stdout.
fn run_waos(folder: &Path, arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_waos"))
        .args(arguments)
        .current_dir(folder)
        .env("CARGO", env!("CARGO"))
        .env("CARGO_TARGET_DIR", target_directory())
        // The profile `waos`'s test programs are built with (`crates/waos/tests/programs`), so the
        // SDK and Waft they built are reused, not built again.
        .env("CARGO_PROFILE_DEV_DEBUG", "line-tables-only")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "waos {}: {}\n{stdout}\n{}",
        arguments.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

/// Patches crates.io, for the programs in `folder`, with this workspace's path of each of its own
/// crates `program`'s manifest names, and gives `program` the workspace's `Cargo.lock`. Waft comes
/// from crates.io, as the workspace's does.
fn build_against_the_workspace(folder: &Path, program: &Path) {
    let workspace = workspace_root();
    let manifest = read_toml(&program.join("Cargo.toml"));
    let mut patches = toml::Table::new();
    for name in manifest["dependencies"].as_table().unwrap().keys() {
        let Some(path) = workspace_path(&workspace, name) else {
            continue;
        };
        let patch =
            toml::Table::from_iter([("path".to_owned(), path.display().to_string().into())]);
        patches.insert(name.clone(), patch.into());
    }
    let crates_io = toml::Table::from_iter([("crates-io".to_owned(), patches.into())]);
    let config = toml::Table::from_iter([("patch".to_owned(), crates_io.into())]);
    fs::create_dir(folder.join(".cargo")).unwrap();
    fs::write(folder.join(".cargo/config.toml"), config.to_string()).unwrap();
    fs::copy(workspace.join("Cargo.lock"), program.join("Cargo.lock")).unwrap();
}

/// The path the workspace's `[workspace.dependencies]` gives the crate `name`, one of its own;
/// none for a crate it takes from crates.io.
fn workspace_path(workspace: &Path, name: &str) -> Option<PathBuf> {
    let manifest = read_toml(&workspace.join("Cargo.toml"));
    let path = manifest["workspace"]["dependencies"]
        .get(name)?
        .get("path")?
        .as_str()?;
    Some(workspace.join(path).canonicalize().unwrap())
}

/// The `src/main.rs` that `template` writes, from this crate's `templates/`.
fn template_main_rs(template: &str) -> String {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("templates")
        .join(format!("{template}.rs"));
    fs::read_to_string(file).unwrap()
}

fn read_toml(path: &Path) -> toml::Table {
    fs::read_to_string(path).unwrap().parse().unwrap()
}

/// The workspace's root, two folders above this crate's.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// Where the programs are built, kept between runs.
fn target_directory() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .expect("CARGO_TARGET_TMPDIR is <target>/tmp")
        .join("test-programs/target")
}
