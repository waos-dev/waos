//! The warning a program hears when it reads the pixels of a hardware buffer it allocated without
//! `cpu-read`, which works only because this system gives the buffer more than was asked, as
//! Apple's systems give any buffer a program allocates every use. The same program is refused on a
//! system that gives a buffer only what was asked, such as Android, so the warning says so, as a
//! `tracing` event under [`targets::PROGRAM`] naming the program.
//!
//! A texture of a buffer the program samples or draws into without having asked is the same trap,
//! and is not warned of: the engine sees the texture imported, not what the program then does with
//! it, and a warning at the import would reach programs that ask for all they use.
//!
//! The invariant: a program hears it once, however many buffers it reads, so the log says what to
//! fix without repeating it at every read.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::targets;

/// Whether a program has been warned that it read a buffer it allocated without `cpu-read`.
#[derive(Default)]
pub(crate) struct UnaskedRead(AtomicBool);

impl UnaskedRead {
    /// Warns `program`, unless it has been warned before.
    pub(crate) fn warn_once(&self, program: &str) {
        if self.0.swap(true, Ordering::Relaxed) {
            return;
        }
        tracing::warn!(
            target: targets::PROGRAM,
            program,
            flag = "cpu-read",
            "the program read the pixels of a hardware buffer it allocated without `cpu-read`: it works on this system, which gives a buffer the program allocates more than it asked, and is refused on systems that give a buffer only what was asked, such as Android",
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    use super::*;

    /// How many events arrived under the program's target.
    #[derive(Clone, Default)]
    struct ProgramEvents(Arc<AtomicUsize>);

    impl Subscriber for ProgramEvents {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.target() == targets::PROGRAM
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, _event: &Event<'_>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    #[test]
    fn a_program_hears_it_once() {
        let events = ProgramEvents::default();
        let unasked_read = UnaskedRead::default();
        tracing::subscriber::with_default(events.clone(), || {
            unasked_read.warn_once("hardware-buffers");
            unasked_read.warn_once("hardware-buffers");
        });
        assert_eq!(events.0.load(Ordering::Relaxed), 1);
    }
}
