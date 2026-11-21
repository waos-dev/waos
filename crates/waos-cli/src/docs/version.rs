//! Which version's docs to serve: the `wasdk` the app in this directory builds with, as its
//! `Cargo.lock` has it, since the docs describe the SDK and WaOS of that release; this
//! `waos`'s own outside an app, or in one that has not locked its `wasdk` yet.

use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use toml_edit::DocumentMut;

/// The version whose docs this directory wants.
pub fn wanted() -> Result<String> {
    let here = env::current_dir().context("reading this directory")?;
    Ok(locked_wasdk(&here).unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned()))
}

/// The version of `wasdk` the nearest `Cargo.lock` at or above `folder` locks.
fn locked_wasdk(folder: &Path) -> Option<String> {
    let lock = folder
        .ancestors()
        .map(|folder| folder.join("Cargo.lock"))
        .find(|lock| lock.is_file())?;
    wasdk_in(&fs::read_to_string(lock).ok()?)
}

/// The version of `wasdk` a `Cargo.lock`'s text locks, if it locks one.
pub fn wasdk_in(lock: &str) -> Option<String> {
    let document: DocumentMut = lock.parse().ok()?;
    document
        .get("package")?
        .as_array_of_tables()?
        .iter()
        .find(|package| package.get("name").and_then(|name| name.as_str()) == Some("wasdk"))?
        .get("version")?
        .as_str()
        .map(str::to_owned)
}
