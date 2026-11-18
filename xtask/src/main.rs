//! `cargo xtask <task>`, the repository's own chores:
//!
//! - `sync-wit` copies `wit/` into each crate that generates bindings from it, replacing the
//!   copy there, after any change to `wit/`.
//! - `check-wit-versions` fails when a WIT package changed since the last release under the
//!   same version, which CI runs on every change.

use std::process::ExitCode;

use xtask::folder_copy::FolderCopy;
use xtask::wit_copies::COPIES;
use xtask::wit_versions;

const USAGE: &str = "usage: cargo xtask sync-wit | check-wit-versions";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("sync-wit") => sync(&COPIES),
        Some("check-wit-versions") => check_wit_versions(),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Makes each of `copies` again from its source, saying each copy made.
fn sync(copies: &[FolderCopy]) -> ExitCode {
    let root = xtask::workspace_root();
    for copy in copies {
        if let Err(error) = copy.sync(&root) {
            eprintln!("copying {} to {}: {error}", copy.source, copy.destination);
            return ExitCode::FAILURE;
        }
        println!("copied {} to {}", copy.source, copy.destination);
    }
    ExitCode::SUCCESS
}

/// Names every WIT package that changed since the last release under the same version, and fails
/// if there is one; before the first release there is nothing to compare.
fn check_wit_versions() -> ExitCode {
    let root = xtask::workspace_root();
    let comparison = wit_versions::compare_with_last_release(&root);
    match comparison {
        Ok(None) => {
            println!("no release tag yet: nothing to compare");
            ExitCode::SUCCESS
        }
        Ok(Some((tag, changed))) if changed.is_empty() => {
            println!("every WIT package is as {tag} has it, or under a new version");
            ExitCode::SUCCESS
        }
        Ok(Some((tag, changed))) => {
            for package in &changed {
                eprintln!(
                    "{package} changed since {tag} under the same version: give it a new one"
                );
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("comparing the WIT with the last release's: {error}");
            ExitCode::FAILURE
        }
    }
}
