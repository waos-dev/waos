//! Builds the example programs the engine's tests run, as components, and writes the path of
//! each into `OUT_DIR/programs.rs` for the crate to include, with each component's code folder,
//! named by its content, which the tests' engines compile it into.
//!
//! The invariants:
//! - The programs are built into a target directory of their own, never the workspace's, which
//!   the build running this script holds locked. The workspace's `Cargo.lock` is shared, and the
//!   outer build has already brought it up to date.
//! - That directory and the code folders are in `<target>/test-programs/`, one place whatever build
//!   runs this script (`cargo test`, `cargo clippy`, another profile), not its `OUT_DIR`, which
//!   differs between them: so the programs are built once, and CI keeps the folder between runs.
//!   `waos-cli`'s `tests/new.rs` builds its templates into the same target directory, with the
//!   same profile settings, so they reuse the SDK and Waft built here.
//! - A code folder is named by its component's content, so a program built again gets a fresh one
//!   and the engine's contract holds: a code folder belongs to one component that never changes.
//!   Folders of components no longer built go.

use std::env;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The programs built, by package name.
const PROGRAMS: [&str; 53] = [
    "survey",
    "forever",
    "forever-shown",
    "trap",
    "triangle",
    "occluded",
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
    "gpu-calls",
    "gpu-errors",
    "gpu-in-block-on",
    "cameras",
    "notes",
    "video-player",
    "input",
    "egui-widgets",
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

/// The components written by hand, in this crate's folder, by the constant `src/lib.rs` names
/// each with.
const WRITTEN: [(&str, &str); 3] = [
    ("INCOMPATIBLE", "incompatible.wat"),
    ("BUILT_AGAINST_2_0", "built-against-2-0.wat"),
    ("IMPORTS_INSPECT", "imports-inspect.wat"),
];

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
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    for (_, file) in WRITTEN {
        println!(
            "cargo:rerun-if-changed={}",
            manifest_dir.join(file).display()
        );
    }
    build_programs(&workspace, &target_dir);
    let built = target_dir.join(TARGET).join("debug");
    let components: Vec<(String, PathBuf)> = PROGRAMS
        .iter()
        .map(|program| {
            let component = built.join(format!("{program}.wasm"));
            (constant_name(program), component)
        })
        .chain(
            WRITTEN
                .iter()
                .map(|(constant, file)| ((*constant).to_owned(), manifest_dir.join(file))),
        )
        .collect();
    let code = test_programs.join("code");
    let folders = code_folders(&components, &code);
    remove_other_folders(&code, &folders);
    write_paths(&out_dir.join("programs.rs"), &components, &folders);
}

/// The target directory of the build running this script, from where cargo puts `out_dir`:
/// `<target>/[<triple>/]<profile>/build/<package>/<hash>/out`, or `…/build/<package>-<hash>/out`
/// before cargo's build-dir layout, the triple there when the build names one. So the profile
/// folder is the one holding the nearest `build`, whichever layout. Not a marker such as
/// `CACHEDIR.TAG`, which cargo writes only into a target directory it makes itself, and CI
/// restores `target/test-programs` before cargo runs.
fn cargo_target_dir(out_dir: &Path) -> PathBuf {
    let profile = out_dir
        .ancestors()
        .find(|folder| folder.file_name() == Some("build".as_ref()))
        .and_then(Path::parent)
        .expect("OUT_DIR is inside <target>/<profile>/build");
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

/// The code folder of each of `components`, in `code`: named by the component's file and a hash
/// of its content.
fn code_folders(components: &[(String, PathBuf)], code: &Path) -> Vec<PathBuf> {
    components
        .iter()
        .map(|(_, component)| {
            let bytes = fs::read(component).expect("a component built or written");
            let mut hasher = DefaultHasher::new();
            bytes.hash(&mut hasher);
            let stem = component.file_stem().unwrap_or_default().to_string_lossy();
            code.join(format!("{stem}-{:016x}", hasher.finish()))
        })
        .collect()
}

/// Removes every folder of `code` but `kept`: the code of components built before.
fn remove_other_folders(code: &Path, kept: &[PathBuf]) {
    let Ok(entries) = fs::read_dir(code) else {
        return;
    };
    for folder in entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
    {
        if !kept.contains(&folder) {
            let _ = fs::remove_dir_all(&folder);
        }
    }
}

/// Writes a constant per program built, naming its component's path, and `CODE`, pairing each
/// component, built or written, with its code folder.
fn write_paths(path: &Path, components: &[(String, PathBuf)], folders: &[PathBuf]) {
    let programs: String = components
        .iter()
        .take(PROGRAMS.len())
        .zip(PROGRAMS)
        .map(|((constant, component), program)| {
            format!(
                "/// `examples/{program}`, built as a component.\npub const {constant}: &str = {:?};\n",
                component.display().to_string()
            )
        })
        .collect();
    let pairs: String = components
        .iter()
        .zip(folders)
        .map(|((constant, _), folder)| {
            format!("    ({constant}, {:?}),\n", folder.display().to_string())
        })
        .collect();
    let code = format!(
        "/// Each component and its code folder.\nconst CODE: [(&str, &str); {}] = [\n{pairs}];\n",
        components.len()
    );
    fs::write(path, programs + &code).expect("OUT_DIR is writable");
}

/// The constant naming `program`'s path: `TWO_WINDOWS` for `two-windows`.
fn constant_name(program: &str) -> String {
    program.to_uppercase().replace('-', "_")
}
