//! The cargo workspace a command runs in: what cargo knows of it, the program's package in it,
//! and `target/waos`, where every command of ours puts what it makes.
//!
//! The invariant: what is known of the workspace comes from cargo, through `cargo metadata`;
//! nothing here reads a manifest or guesses the target directory's place.

use std::path::PathBuf;

use anyhow::{Context, Result};
use cargo_metadata::{Metadata, MetadataCommand, Package};

/// What cargo knows of the workspace of this directory.
pub fn metadata() -> Result<Metadata> {
    MetadataCommand::new()
        .exec()
        .context("reading what cargo knows of this project")
}

/// The workspace's package `name` names, or the package of this directory with none.
pub fn chosen_package<'a>(metadata: &'a Metadata, name: Option<&str>) -> Result<&'a Package> {
    match name {
        Some(name) => metadata
            .packages
            .iter()
            .find(|package| {
                package.name.as_str() == name && metadata.workspace_members.contains(&package.id)
            })
            .with_context(|| format!("no package named {name} in this workspace")),
        None => metadata
            .root_package()
            .context("no package in this directory: say which program with -p <package>"),
    }
}

/// `target/waos`, where our commands put what they make.
pub fn waos_folder(metadata: &Metadata) -> PathBuf {
    metadata.target_directory.join("waos").into_std_path_buf()
}
