//! The engine's program log: a process-wide `tracing` subscriber recording only events under
//! `waos::targets::PROGRAM`, each line with the name of the program that wrote it.
//!
//! The invariant: each test that reads this log runs its program under a name of its own
//! (`Program::named`), so it reads only its own lines and tests run side by side without seeing
//! each other.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

use super::WITHIN;

/// The lines every program wrote to the engine's log, each paired with its program's name.
#[derive(Default)]
pub struct EngineLog {
    lines: Mutex<Vec<(String, String)>>,
    /// Woken as each program line is logged.
    logged: Condvar,
}

/// The engine's program log. Installs this test binary's one `tracing` subscriber the first time
/// it is asked for.
pub fn engine_log() -> &'static EngineLog {
    static LOG: OnceLock<Arc<EngineLog>> = OnceLock::new();
    LOG.get_or_init(|| {
        let log = Arc::new(EngineLog::default());
        tracing::subscriber::set_global_default(Arc::clone(&log))
            .expect("the engine log is the first tracing subscriber");
        log
    })
}

impl EngineLog {
    /// Every line `program` wrote to the engine's log so far, in order.
    pub fn lines_of(&self, program: &str) -> Vec<String> {
        lines_of(&self.lines(), program)
    }

    /// Waits until `program` has written `line` to the engine's log, failing after [`WITHIN`] with
    /// what it wrote.
    pub fn wait_for(&self, program: &str, line: &str) {
        let deadline = Instant::now() + WITHIN;
        let mut lines = self.lines();
        loop {
            if lines
                .iter()
                .any(|(name, written)| name == program && written == line)
            {
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "{program} never logged {line:?}; it logged {:#?}",
                lines_of(&lines, program)
            );
            lines = self
                .logged
                .wait_timeout(lines, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn lines(&self) -> MutexGuard<'_, Vec<(String, String)>> {
        self.lines.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Subscriber for EngineLog {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_event() && metadata.target() == waos::targets::PROGRAM
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        if event.metadata().target() != waos::targets::PROGRAM {
            return;
        }
        let mut fields = ProgramLine::default();
        event.record(&mut fields);
        if let (Some(program), Some(line)) = (fields.program, fields.message) {
            self.lines().push((program, line));
            self.logged.notify_all();
        }
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

/// The two fields of a program's output event.
#[derive(Default)]
struct ProgramLine {
    program: Option<String>,
    message: Option<String>,
}

impl Visit for ProgramLine {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match field.name() {
            "program" => self.program = Some(format!("{value:?}")),
            "message" => self.message = Some(format!("{value:?}")),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "program" {
            self.program = Some(value.to_owned());
        }
    }
}

fn lines_of(lines: &[(String, String)], program: &str) -> Vec<String> {
    lines
        .iter()
        .filter(|(name, _)| name == program)
        .map(|(_, line)| line.clone())
        .collect()
}
