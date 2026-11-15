//! Builds the example programs the engine's tests run, as components, and writes the path of
//! each into `OUT_DIR/programs.rs` for the crate to include, with the folder beside them that the
//! tests' engines keep their compiled code in.
//!
//! The invariant: the programs are built into a target directory of this crate's own, under its
//! `OUT_DIR`, never the workspace's, which the build running this script holds locked. The
//! workspace's `Cargo.lock` is shared, and the outer build has already brought it up to date.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The programs built, by package name.
const PROGRAMS: [&str; 36] = [
    "survey",
    "forever",
    "trap",
    "triangle",
    "composition-layers",
    "fonts",
    "gallery",
    "two-windows",
    "permissions",
    "files",
    "file-escapes",
    "network",
    "fetch",
    "trust",
    "clipboard",
    "copy-paste",
    "menus",
    "waft-menus",
    "sound",
    "tone",
    "file-dialogs",
    "opener",
    "notifications",
    "machines",
    "media-players",
    "media-assets",
    "image-codecs",
    "hardware-buffers",
    "video-frames",
    "cameras",
    "notes",
    "video-player",
    "input",
    "back",
    "waft-back",
    "workers",
];

/// What a program is built for.
const TARGET: &str = "wasm32-wasip3";

/// What the programs are built from, from the workspace's root: its programs, the SDK and the
/// WIT, the versions of what they depend on, and the crates of Waft's checkout beside it, which
/// the gallery, two-windows, copy-paste, waft-menus, waft-back, notes and video-player are built
/// from.
const WATCHED: [&str; 7] = [
    "builtin-apps",
    "examples",
    "sdk",
    "wit",
    "Cargo.lock",
    "../waft/crates",
    "../waft/examples",
];

fn main() {
    let workspace = workspace_root();
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let target_dir = out_dir.join("target");
    for watched in WATCHED {
        println!(
            "cargo:rerun-if-changed={}",
            workspace.join(watched).display()
        );
    }
    build_programs(&workspace, &target_dir);
    write_paths(
        &out_dir.join("programs.rs"),
        &target_dir,
        &out_dir.join("code-cache"),
    );
}

/// The workspace's root: four directories above this crate's.
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    manifest_dir
        .ancestors()
        .nth(4)
        .expect("this crate is at crates/waos/tests/programs")
        .to_path_buf()
}

/// Builds every program with the cargo running this script, for its target, into `target_dir`.
fn build_programs(workspace: &Path, target_dir: &Path) {
    let cargo = env::var_os("CARGO").expect("cargo sets CARGO");
    let mut build = Command::new(cargo);
    build
        .current_dir(workspace)
        .arg("build")
        .arg("--target")
        .arg(TARGET)
        .arg("--target-dir")
        .arg(target_dir)
        // The flags of the build running this script are for its own target, not the programs'.
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        // Line tables are what a trap's backtrace names files and lines from; the rest of the
        // debug information, which nothing in a test reads, was half of forever's component and
        // three quarters of the gallery's.
        .env("CARGO_PROFILE_DEV_DEBUG", "line-tables-only");
    // Cargo tells this script its own target's cfgs (`CARGO_CFG_WINDOWS`, …) and sets only those of
    // a target that has them for the build scripts of the programs' build, so a Windows host's would
    // reach them as the programs' own.
    for (key, _) in env::vars_os() {
        if key.to_string_lossy().starts_with("CARGO_CFG_") {
            build.env_remove(key);
        }
    }
    for program in PROGRAMS {
        build.arg("--package").arg(program);
    }
    let status = build.status().expect("cargo runs");
    assert!(
        status.success(),
        "building the test programs failed: {status}"
    );
}

/// Writes a constant per program, naming its component's path, and `CODE_CACHE`, naming
/// `code_cache`.
fn write_paths(path: &Path, target_dir: &Path, code_cache: &Path) {
    let built = target_dir.join(TARGET).join("debug");
    let programs: String = PROGRAMS
        .iter()
        .map(|program| {
            let component = built.join(format!("{program}.wasm"));
            format!(
                "/// `examples/{program}`, built as a component.\npub const {}: &str = {:?};\n",
                constant_name(program),
                component.display().to_string()
            )
        })
        .collect();
    let cache = format!(
        "/// Where the tests' engines keep the code compiled from these programs.\npub const CODE_CACHE: &str = {:?};\n",
        code_cache.display().to_string()
    );
    fs::write(path, programs + &cache).expect("OUT_DIR is writable");
}

/// The constant naming `program`'s path: `TWO_WINDOWS` for `two-windows`.
fn constant_name(program: &str) -> String {
    program.to_uppercase().replace('-', "_")
}
