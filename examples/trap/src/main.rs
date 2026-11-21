//! A program that traps: it asks for a view, as a program shown in a window does, then aborts at
//! once, which in a component is the `unreachable` instruction, as a Rust panic ends in. What the
//! engine's tests see end in failure, without having returned, and what WaOS shows a program
//! that failed in its tab or window.
//!
//! ```text
//! cargo build -p trap --target wasm32-wasip3
//! ```

use wasdk::views;
use wasdk::windows::WindowConfig;

#[wasdk::main]
async fn main() {
    // Whether or not a view comes, the program fails the same way.
    let _ = views::request_view(&WindowConfig::new(), None);
    std::process::abort();
}
