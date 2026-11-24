//! Builds the example programs the engine's tests run, as components, and writes the path of
//! each into `OUT_DIR/programs.rs` for the crate to include, with the folder beside them that the
//! tests' engines keep their compiled code in.
//!
//! The invariants:
//! - The programs are built into a target directory of their own, never the workspace's, which
//!   the build running this script holds locked. The workspace's `Cargo.lock` is shared, and the
//!   outer build has already brought it up to date.
//! - That directory and the code cache are `<target>/test-programs/`, one place whatever build
//!   runs this script (`cargo test`, `cargo clippy`, another profile), not its `OUT_DIR`, which
//!   differs between them: so the programs are built once, and CI keeps the folder between runs.
//!   `waos-cli`'s `tests/new.rs` builds its templates into the same target directory, with the
//!   same profile settings, so they reuse the SDK and Waft built here.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The programs built, by package name.
const PROGRAMS: [&str; 48] = [
    "survey",
    "forever",
    "forever-shown",
    "trap",
    "triangle",
    "composition-layers",
    "fonts",
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
    "file-watching",
    "media-players",
    "media-assets",
    "image-codecs",
    "hardware-buffers",
    "video-frames",
    "video-decoders",
    "video-decoding",
    "readback",
    "naga-shader",
    "cameras",
    "notes",
    "video-player",
    "input",
    "back",
    "waft-back",
    "waft-image",
    "waft-http",
    "waft-time",
    "workers",
    "inspect",
    "environment",
    "arguments",
    "flood",
];

/// What a program is built for.
const TARGET: &str = "wasm32-wasip3";

/// What the programs are built from, from the workspace's root: its programs, the SDK and the
/// WIT, and the versions of what they depend on, Waft's among them.
const WATCHED: [&str; 5] = ["builtin-apps", "examples", "sdk", "wit", "Cargo.lock"];

fn main() {
    let workspace = workspace_root();
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let test_programs = cargo_target_dir(&out_dir).join("test-programs");
    let target_dir = test_programs.join("target");
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
        &test_programs.join("code-cache"),
    );
}

/// The target directory of the build running this script, from where cargo puts `out_dir`:
/// `<target>/[<triple>/]<profile>/build/<package>-<hash>/out`, the triple there when the build
/// names one. Not a marker such as `CACHEDIR.TAG`, which cargo writes only into a target
/// directory it makes itself, and CI restores `target/test-programs` before cargo runs.
fn cargo_target_dir(out_dir: &Path) -> PathBuf {
    let profile = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR is <target>/<profile>/build/<package>/out");
    let above_profile = profile
        .parent()
        .expect("a profile folder is inside the target directory");
    let triple = env::var_os("TARGET").expect("cargo sets TARGET");
    if above_profile.file_name() == Some(triple.as_os_str()) {
        above_profile
            .parent()
            .expect("a triple folder is inside the target directory")
            .to_path_buf()
    } else {
        above_profile.to_path_buf()
    }
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
        // most of a Waft app's.
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
