//! A Waft app that reads a local `DateTime`, over `wasdk-adapter-waft`: the midnight of 2026-01-01
//! on the user's clock, and the same instant in UTC. It writes no time zone code of its own:
//! Waft's `DateTime` asks the platform for the offset and the name at each read, which the adapter
//! answers from the system's `utc-offset` and `time-zone-abbreviation`.
//!
//! It prints the local hour and zone name, and the UTC hour and the offset from UTC, as
//! `local: 0 CET` and `utc: 23, offset 1 h`.
//!
//! ```text
//! cargo build -p waft-time --target wasm32-wasip3
//! waos run -p waft-time
//! ```

use std::cell::Cell;
use std::rc::Rc;

use waft_embedder::{Color, WindowConfig};
use waft_foundation::{App, DateTime};
use waft_widgets::{Builder, ColoredBox, IntoWidget, run_app_with};

/// What the app draws, which is nothing.
const BACKGROUND: Color = Color::new(0xFFFFFFFF);

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        let window = WindowConfig {
            title: "Waft time".to_owned(),
            size: [320.0, 240.0],
            ..WindowConfig::default()
        };
        let printed = Rc::new(Cell::new(false));
        let page = Builder::new(move |app, _context| {
            if !printed.replace(true) {
                print_local_time(app);
            }
            ColoredBox::new(BACKGROUND).into_widget()
        });
        run_app_with(app, page.into_widget(), window);
    })
    .await;
}

/// Prints the midnight of 2026-01-01 on the user's clock, and that instant in UTC.
fn print_local_time(app: &mut App) {
    let local = DateTime::new(app, 2026, 1, 1);
    let offset = local.time_zone_offset(app);
    println!("local: {} {}", local.hour(app), local.time_zone_name(app));
    println!(
        "utc: {}, offset {} h",
        local.to_utc().hour(app),
        offset.in_hours()
    );
}
