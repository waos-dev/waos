//! GPUI's and the program's `log` records, on the program's standard error, which `cargo waos run`
//! shows and WaOS keeps in its log otherwise, filtered by `RUST_LOG` with `env_filter`, the crate
//! env_logger filters with: `debug` for every target, `info,gpui=debug` for gpui's debug records
//! among the rest, and a target alone for every level of it; Info for every target with none.
//!
//! The invariant: the filter is read once, at the program's start, from the environment WaOS gave
//! the program, so `cargo waos run --env RUST_LOG=debug` reaches it.

use std::env;
use std::sync::OnceLock;

use log::{LevelFilter, Log, Metadata, Record};

/// The program's log, on its standard error.
struct StandardError;

impl Log for StandardError {
    fn enabled(&self, metadata: &Metadata) -> bool {
        filter().is_some_and(|filter| filter.enabled(metadata))
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

/// The filter `RUST_LOG` asked for, once the program has started.
static FILTER: OnceLock<env_filter::Filter> = OnceLock::new();

fn filter() -> Option<&'static env_filter::Filter> {
    FILTER.get()
}

pub(crate) fn init() {
    let mut builder = env_filter::Builder::new();
    match env::var("RUST_LOG") {
        Ok(rust_log) => {
            builder.parse(&rust_log);
        }
        // Nothing said: every target at Info, as the adapter logged always.
        Err(_) => {
            builder.filter_level(LevelFilter::Info);
        }
    }
    let filter = builder.build();
    let highest = filter.filter();
    let _ = FILTER.set(filter);
    if log::set_logger(&StandardError).is_ok() {
        log::set_max_level(highest);
    }
}
