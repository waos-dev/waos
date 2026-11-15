//! GPUI's and the program's `log` records, on the program's standard error, which `cargo waos run`
//! shows and WaOS keeps in its log otherwise.

use log::{Level, LevelFilter, Log, Metadata, Record};

struct StandardError;

impl Log for StandardError {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

pub(crate) fn init() {
    if log::set_logger(&StandardError).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
}
