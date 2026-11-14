//! `cargo xtask <task>`, the repository's own chores:
//!
//! - `sync-wit` copies `wit/` into each crate that generates bindings from it, replacing the
//!   copy there, after any change to `wit/`.

use std::process::ExitCode;

use xtask::wit_copies::COPIES;

const USAGE: &str = "usage: cargo xtask sync-wit";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("sync-wit") => sync_wit(),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Copies `wit/` again into every crate that carries a copy, saying each copy made.
fn sync_wit() -> ExitCode {
    let root = xtask::workspace_root();
    for copy in &COPIES {
        if let Err(error) = copy.sync(&root) {
            eprintln!("copying {} to {}: {error}", copy.source, copy.destination);
            return ExitCode::FAILURE;
        }
        println!("copied {} to {}", copy.source, copy.destination);
    }
    ExitCode::SUCCESS
}
