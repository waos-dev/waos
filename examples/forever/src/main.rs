//! A program that never returns: it counts, and keeps counting, saying its count every
//! [`REPORT_EVERY`] steps and asking nothing else of the platform. What the engine's tests stop
//! from outside, and what they see a freeze on: the reports stop, and go on from the same count
//! after the thaw.
//!
//! ```text
//! cargo build -p forever --target wasm32-wasip3
//! ```

/// How many steps between two reports: often enough that a test sees one within a fraction of a
/// second, rare enough that the loop is what the program does.
const REPORT_EVERY: u64 = 1 << 20;

#[wasdk::main]
async fn main() {
    let mut count: u64 = 0;
    loop {
        count = std::hint::black_box(count.wrapping_add(1));
        if count.is_multiple_of(REPORT_EVERY) {
            println!("count {count}");
        }
    }
}
