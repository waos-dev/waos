//! A program that says its arguments, one a line, starting with its own name, then what
//! whoever ran it gave after `--`: `waos run -- one two`.
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
