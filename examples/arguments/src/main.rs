//! A program that says its arguments, one a line, starting with its own name, then what
//! whoever ran it gave after `--`: `cargo waos run -- one two` (decision 68 of
//! `docs/proposals/2026-10-17-the-waos-project.md` gives a run its environment and arguments).
//!
//! ```text
//! cargo build -p arguments --target wasm32-wasip3
//! ```

#[wasdk::main]
async fn main() {
    for argument in std::env::args() {
        println!("argument: {argument}");
    }
}
