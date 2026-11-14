//! The components `cargo waos` sent, each kept in a file of the app's own named by its program's
//! id, where the engine compiles it and loads it from.
//!
//! The invariant: one run of an id holds the id's file at a time, from before it writes the file
//! until it lets go, which a run does once it is among the runs. So a later run of the id never
//! writes over a file an earlier one's compile is reading, and the runs of an id start in the
//! order they came. Runs of different ids hold nothing of each other's: one's compile holds back
//! no other's.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result};
use tokio::sync::OwnedMutexGuard;
use waos_package::ProgramId;

/// The files of the components sent, by the id each was sent as.
pub(crate) struct SentComponents {
    folder: PathBuf,
    /// Each id's turn at its file.
    turns: Mutex<HashMap<ProgramId, Arc<tokio::sync::Mutex<()>>>>,
    /// What a test holds every write back with, so that it sees a run being loaded for as long
    /// as it likes: a load is quicker than what a test does to see it.
    #[cfg(test)]
    write_hold: Arc<WriteHold>,
}

/// The file of the component sent as one id, held for one run until dropped.
pub(crate) struct SentComponent {
    path: PathBuf,
    _turn: OwnedMutexGuard<()>,
    #[cfg(test)]
    write_hold: Arc<WriteHold>,
}

impl SentComponents {
    /// The components sent, kept in `folder`.
    pub(crate) fn new(folder: PathBuf) -> SentComponents {
        SentComponents {
            folder,
            turns: Mutex::default(),
            #[cfg(test)]
            write_hold: Arc::default(),
        }
    }

    /// The file of the component sent as the program `id`, once no other run of the id holds it.
    pub(crate) async fn hold(&self, id: &ProgramId) -> SentComponent {
        let turn = self.turn_of(id).lock_owned().await;
        SentComponent {
            path: component_path(&self.folder, id),
            _turn: turn,
            #[cfg(test)]
            write_hold: Arc::clone(&self.write_hold),
        }
    }

    /// Whether a component was sent as the program `id` and is kept still: a run of `cargo waos
    /// run` on this machine ran it.
    pub(crate) fn was_sent(&self, id: &ProgramId) -> bool {
        component_path(&self.folder, id).is_file()
    }

    fn turn_of(&self, id: &ProgramId) -> Arc<tokio::sync::Mutex<()>> {
        // Every change leaves the list whole, so one that panicked leaves nothing half-changed.
        let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(turns.entry(id.clone()).or_default())
    }

    /// Holds every write back until the answer is dropped.
    #[cfg(test)]
    pub(crate) fn hold_writes(&self) -> HeldWrites {
        *self.write_hold.held.lock().unwrap() = true;
        HeldWrites(Arc::clone(&self.write_hold))
    }
}

impl SentComponent {
    /// Where the component is kept.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `component` as the file, in place of what was sent as the id before. Blocks, so
    /// call it off the runtime's thread.
    pub(crate) fn write(&self, component: &[u8]) -> Result<()> {
        #[cfg(test)]
        self.write_hold.wait_while_held();
        if let Some(folder) = self.path.parent() {
            fs::create_dir_all(folder).with_context(|| format!("making {}", folder.display()))?;
        }
        fs::write(&self.path, component).with_context(|| format!("writing {}", self.path.display()))
    }
}

/// Where the component sent as the program `id` is kept: a file named by the id, which an id's
/// letters, digits and dots always make one.
fn component_path(components: &Path, id: &ProgramId) -> PathBuf {
    components.join(format!("{id}.wasm"))
}

/// Whether a test holds the writes back, and how many have waited for it to let go.
#[cfg(test)]
#[derive(Default)]
struct WriteHold {
    held: Mutex<bool>,
    released: std::sync::Condvar,
    waited: tokio::sync::watch::Sender<usize>,
}

#[cfg(test)]
impl WriteHold {
    fn wait_while_held(&self) {
        let held = self.held.lock().unwrap();
        if *held {
            self.waited.send_modify(|waited| *waited += 1);
        }
        drop(self.released.wait_while(held, |held| *held).unwrap());
    }
}

/// The writes held back, until this is dropped.
#[cfg(test)]
pub(crate) struct HeldWrites(Arc<WriteHold>);

#[cfg(test)]
impl HeldWrites {
    /// Resolves once a write waits, or has waited, to be let go.
    pub(crate) async fn a_write_waits(&self) {
        let mut waited = self.0.waited.subscribe();
        waited.wait_for(|waited| *waited > 0).await.unwrap();
    }
}

#[cfg(test)]
impl Drop for HeldWrites {
    fn drop(&mut self) {
        *self.0.held.lock().unwrap() = false;
        self.0.released.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn survey() -> ProgramId {
        "dev.waos.examples.survey".parse().unwrap()
    }

    #[test]
    fn a_component_is_kept_under_its_programs_id() {
        assert_eq!(
            component_path(Path::new("/caches/dev"), &survey()),
            Path::new("/caches/dev/dev.waos.examples.survey.wasm")
        );
    }

    #[tokio::test]
    async fn a_later_run_of_an_id_waits_for_the_file_and_another_ids_does_not() {
        let folder = tempfile::tempdir().unwrap();
        let components = SentComponents::new(folder.path().to_owned());
        let survey = survey();
        let first = components.hold(&survey).await;
        let other: ProgramId = "dev.waos.examples.forever".parse().unwrap();
        components.hold(&other).await;
        let later = tokio::time::timeout(Duration::from_millis(50), components.hold(&survey));
        assert!(later.await.is_err(), "held by the first");
        drop(first);
        components.hold(&survey).await;
    }
}
