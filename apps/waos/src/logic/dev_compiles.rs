//! The programs `cargo waos` is compiling now: a compile the dev server makes on a thread of its
//! own, begun and ended around it, listed as the set whoever shows it reads. A dev program is not
//! installed, so its compiles are apart from the library's (`InstalledCode`), and both are named
//! by a program's id.
//!
//! The invariant: an id is in the set exactly while a compile of it runs, however many runs of the
//! program wait for it.

#![cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "reported where macOS's activity follows it; kept here so the dev server is one code path"
    )
)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::watch;
use waos_package::ProgramId;

/// The programs `cargo waos` is compiling, and each change of them.
pub(crate) struct DevCompiles {
    /// How many compiles of each id run, so one ending leaves another still listed.
    running: Mutex<BTreeMap<ProgramId, usize>>,
    changes: watch::Sender<BTreeSet<ProgramId>>,
}

impl DevCompiles {
    /// The compiles to report, and the programs compiling for whoever follows them from now on.
    pub(crate) fn new() -> (Arc<DevCompiles>, watch::Receiver<BTreeSet<ProgramId>>) {
        let changes = watch::Sender::default();
        let compiles = Arc::new(DevCompiles {
            running: Mutex::default(),
            changes,
        });
        let now = compiles.changes.subscribe();
        (compiles, now)
    }

    /// The compile of the program `id`'s code begins: it is listed as compiling from now until the
    /// answer is dropped.
    pub(crate) fn compiling(self: &Arc<Self>, id: &ProgramId) -> DevCompile {
        {
            let mut compiling = self.running();
            *compiling.entry(id.clone()).or_default() += 1;
            self.tell(&compiling);
        }
        DevCompile {
            compiles: Arc::clone(self),
            id: id.clone(),
        }
    }

    fn running(&self) -> MutexGuard<'_, BTreeMap<ProgramId, usize>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn tell(&self, compiling: &BTreeMap<ProgramId, usize>) {
        self.changes
            .send_replace(compiling.keys().cloned().collect());
    }
}

/// A dev compile under way, which ends when dropped.
pub(crate) struct DevCompile {
    compiles: Arc<DevCompiles>,
    id: ProgramId,
}

impl Drop for DevCompile {
    fn drop(&mut self) {
        let mut compiling = self.compiles.running();
        let Some(running) = compiling.get_mut(&self.id) else {
            return;
        };
        *running -= 1;
        if *running == 0 {
            compiling.remove(&self.id);
        }
        self.compiles.tell(&compiling);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_is_listed_while_its_compile_runs() {
        let (compiles, mut ids) = DevCompiles::new();
        let id: ProgramId = "dev.example.program".parse().unwrap();
        assert!(ids.borrow().is_empty());

        let first = compiles.compiling(&id);
        assert!(ids.borrow_and_update().contains(&id));

        let second = compiles.compiling(&id);
        drop(first);
        assert!(
            ids.borrow_and_update().contains(&id),
            "another compile of it still runs"
        );

        drop(second);
        assert!(!ids.borrow_and_update().contains(&id));
    }
}
