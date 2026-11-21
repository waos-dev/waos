//! A program that says the variables its environment holds, each as `NAME=VALUE`, in the order
//! of their names, and `none` where it holds none: `TMPDIR`, which WaOS sets, and what a
//! `waos run --env NAME=VALUE` added to that run alone (the design's decision 68).
//!
//! ```text
//! cargo build -p environment --target wasm32-wasip3
//! ```

#[wasdk::main]
async fn main() {
    let mut variables: Vec<(String, String)> = std::env::vars().collect();
    variables.sort();
    if variables.is_empty() {
        println!("none");
        return;
    }
    for (name, value) in variables {
        println!("{name}={value}");
    }
}
