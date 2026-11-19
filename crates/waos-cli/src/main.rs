//! Standalone `cargo-waos` binary: delegates to `waos_cli` with [`waos_cli::DefaultStarter`].

use std::process::ExitCode;

fn main() -> ExitCode {
    waos_cli::run_from_env(&waos_cli::DefaultStarter)
}
