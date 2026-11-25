//! Forever with a view: it asks for one view, as a program shown in a window does, draws nothing
//! in it, and counts, and keeps counting, saying its count every [`REPORT_EVERY`] steps. What the
//! engine's tests see a freeze on, since a program is frozen for its views being out of sight:
//! its view hidden, the reports stop, and go on from the same count after the unfreeze. `forever`
//! is the same with no view, a program nothing freezes.
//!
//! ```text
//! cargo build -p forever-shown --target wasm32-wasip3
//! ```

use wasdk::views;
use wasdk::windows::WindowConfig;

/// How many steps between two reports: often enough that a test sees one within a fraction of a
/// second, rare enough that the loop is what the program does.
const REPORT_EVERY: u64 = 1 << 20;

#[wasdk::main]
async fn main() {
    if let Err(error) = views::request_view(&WindowConfig::new(), None) {
        println!("no view: {error}");
    }
    let mut count: u64 = 0;
    loop {
        count = std::hint::black_box(count.wrapping_add(1));
        if count.is_multiple_of(REPORT_EVERY) {
            println!("count {count}");
        }
    }
}
