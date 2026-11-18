//! GPUI's and the program's `log` records, on the program's standard error, which `cargo waos run`
//! shows and WaOS keeps in its log otherwise, filtered as `RUST_LOG` asks: `debug` for every
//! target, `info,gpui=debug` for gpui's debug records among the rest, and a target alone for every
//! level of it; Info for every target with none.
//!
//! The invariant: the filter is read once, at the program's start, from the environment WaOS gave
//! the program, so `cargo waos run --env RUST_LOG=debug` reaches it.

use std::env;
use std::sync::OnceLock;

use log::{Level, LevelFilter, Log, Metadata, Record};

/// The level each target is kept at, from `RUST_LOG`.
struct Filter {
    /// The level for a target no part names.
    default: LevelFilter,
    /// The level of each target a part names, by target or the start of one.
    targets: Vec<(String, LevelFilter)>,
}

impl Filter {
    /// The filter the variable `rust_log` asks for, or Info for none; parts that name no level are
    /// left out.
    fn parse(rust_log: Option<&str>) -> Filter {
        let mut filter = Filter {
            default: LevelFilter::Info,
            targets: Vec::new(),
        };
        let parts = rust_log
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty());
        for part in parts {
            match part.split_once('=') {
                Some((target, level)) => {
                    if let Ok(level) = level.trim().parse() {
                        filter.targets.push((target.trim().to_owned(), level));
                    }
                }
                None => match part.parse::<LevelFilter>() {
                    Ok(level) => filter.default = level,
                    // A target alone is every level of it, as env_logger's filters read one.
                    Err(_) => filter.targets.push((part.to_owned(), LevelFilter::Trace)),
                },
            }
        }
        filter
    }

    /// Whether a record of `level` from `target` is kept.
    fn enabled(&self, level: Level, target: &str) -> bool {
        level <= self.level_of(target)
    }

    /// The highest level records from `target` are kept at: the longest part naming a start of it,
    /// or the default.
    fn level_of(&self, target: &str) -> LevelFilter {
        self.targets
            .iter()
            .filter(|(named, _)| target.starts_with(named.as_str()))
            .max_by_key(|(named, _)| named.len())
            .map_or(self.default, |(_, level)| *level)
    }

    /// The highest level any target is kept at, which `log` needs before it checks a record.
    fn highest(&self) -> LevelFilter {
        self.targets
            .iter()
            .map(|(_, level)| *level)
            .fold(self.default, std::cmp::max)
    }
}

/// The program's log, on its standard error.
struct StandardError;

impl Log for StandardError {
    fn enabled(&self, metadata: &Metadata) -> bool {
        filter().is_some_and(|filter| filter.enabled(metadata.level(), metadata.target()))
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

/// The filter `RUST_LOG` asked for, once the program has started.
static FILTER: OnceLock<Filter> = OnceLock::new();

/// The filter `init` set.
fn filter() -> Option<&'static Filter> {
    FILTER.get()
}

pub(crate) fn init() {
    let filter = Filter::parse(env::var("RUST_LOG").ok().as_deref());
    let highest = filter.highest();
    let _ = FILTER.set(filter);
    if log::set_logger(&StandardError).is_ok() {
        log::set_max_level(highest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_log_is_a_level_a_targets_level_or_a_target_for_every_level() {
        let filter = Filter::parse(Some("info,gpui=debug,quiet_crate"));
        assert_eq!(filter.default, LevelFilter::Info);
        assert!(filter.enabled(Level::Info, "gpui"));
        assert!(filter.enabled(Level::Debug, "gpui::window"));
        assert!(!filter.enabled(Level::Trace, "gpui"));
        assert!(filter.enabled(Level::Error, "quiet_crate"));
        assert!(filter.enabled(Level::Trace, "quiet_crate"));
        assert!(!filter.enabled(Level::Debug, "other"));
        assert_eq!(filter.highest(), LevelFilter::Trace);
    }

    #[test]
    fn the_longest_target_named_answers_and_info_is_the_default() {
        let filter = Filter::parse(Some("debug,gpui=error,gpui::window=trace"));
        assert!(filter.enabled(Level::Debug, "notes"));
        assert!(!filter.enabled(Level::Info, "gpui"));
        assert!(filter.enabled(Level::Error, "gpui::window"));
        assert!(filter.enabled(Level::Trace, "gpui::window::inner"));

        let none = Filter::parse(None);
        assert_eq!(none.default, LevelFilter::Info);
        assert!(!none.enabled(Level::Debug, "anything"));
    }
}
