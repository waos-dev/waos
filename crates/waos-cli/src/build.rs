//! Building a program as a component: cargo for `wasm32-wasip2`, then the component put where
//! every command of ours finds it, `target/waos/<profile>/<package>.wasm`. A development build
//! carries function names and line tables only, which is what WaOS reads of debug info.
//!
//! The invariant: cargo says where it put the component, through its JSON messages; nothing
//! here guesses the target directory's layout.

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use cargo_metadata::{Artifact, Message, Metadata, Package, TargetKind};

use crate::workspace;

/// What a program is built for.
const TARGET: &str = "wasm32-wasip2";

/// Which of cargo's profiles a program is built with.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// For development: function names and line tables, unoptimized.
    Debug,
    /// Optimized, as a package carries it.
    Release,
}

impl Profile {
    /// The profile's folder, as cargo names its own.
    fn folder(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

/// A program built as a component.
pub struct BuiltProgram {
    /// Its package's name, which names it in WaOS.
    pub name: String,
    /// The component, under `target/waos`.
    pub component: PathBuf,
}

/// Builds `package`, a package of the workspace `metadata` describes, as a component with
/// `profile`.
pub fn build(metadata: &Metadata, package: &Package, profile: Profile) -> Result<BuiltProgram> {
    let built = cargo_build(metadata, package, profile)?;
    let component = workspace::waos_folder(metadata)
        .join(profile.folder())
        .join(format!("{}.wasm", package.name));
    place(&built, &component)?;
    Ok(BuiltProgram {
        name: package.name.to_string(),
        component,
    })
}

/// `cargo build` of `package` for the target, answering the component cargo made.
fn cargo_build(metadata: &Metadata, package: &Package, profile: Profile) -> Result<PathBuf> {
    let mut cargo = Command::new(cargo_binary());
    cargo.current_dir(&metadata.workspace_root).args([
        "build",
        "--target",
        TARGET,
        "--message-format=json-render-diagnostics",
        "--package",
        &package.name,
    ]);
    if profile == Profile::Release {
        cargo.arg("--release");
    }
    // A development build keeps function names and line tables, which trap backtraces read, and
    // leaves out the types and variables only a source-level debugger reads: a fifth of the size
    // (the gallery: 102 MB against 442). The program's own manifest is not touched.
    cargo.env("CARGO_PROFILE_DEV_DEBUG", "line-tables-only");
    let mut child = cargo
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("running cargo")?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let mut component = None;
    for message in Message::parse_stream(BufReader::new(stdout)) {
        if let Message::CompilerArtifact(artifact) = message.context("reading cargo's messages")?
            && artifact.package_id == package.id
        {
            component = component_of(&artifact).or(component);
        }
    }
    let status = child.wait().context("waiting for cargo")?;
    if !status.success() {
        bail!("cargo build failed");
    }
    component.with_context(|| {
        format!(
            "cargo built no component for {}: a program is a package with a binary target",
            package.name
        )
    })
}

/// The cargo that runs this command, which set `CARGO`, or the one on the path.
fn cargo_binary() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}

/// The component among what one compilation of the program made: the `.wasm` of its binary or
/// its `cdylib`, not a build script's.
fn component_of(artifact: &Artifact) -> Option<PathBuf> {
    let kinds = &artifact.target.kind;
    if !kinds.contains(&TargetKind::Bin) && !kinds.contains(&TargetKind::CDyLib) {
        return None;
    }
    artifact
        .executable
        .clone()
        .or_else(|| {
            artifact
                .filenames
                .iter()
                .find(|file| file.extension() == Some("wasm"))
                .cloned()
        })
        .map(|path| path.into_std_path_buf())
}

/// Puts a copy of the component `built` at `component`.
fn place(built: &Path, component: &Path) -> Result<()> {
    if let Some(folder) = component.parent() {
        fs::create_dir_all(folder).with_context(|| format!("making {}", folder.display()))?;
    }
    fs::copy(built, component)
        .with_context(|| format!("copying {} to {}", built.display(), component.display()))?;
    Ok(())
}
