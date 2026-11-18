//! The repository's own chores, which `cargo xtask` runs from its checkout and its tests check.

pub mod docs_copy;
pub mod folder_copy;
pub mod wit_copies;
pub mod wit_versions;

use std::path::{Path, PathBuf};

/// The workspace's root, the folder above this crate's.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is a folder of the workspace's root")
        .to_path_buf()
}
