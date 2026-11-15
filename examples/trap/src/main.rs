//! A program that traps: it aborts at once, which in a component is the `unreachable`
//! instruction, as a Rust panic ends in. What the engine's tests see end in failure, without
//! having returned.
//!
//! ```text
//! cargo build -p trap --target wasm32-wasip3
//! ```

#[wasdk::main]
async fn main() {
    std::process::abort();
}
