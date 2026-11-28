//! The opens under way, by app, each with its stage, as their watchers hear them: an open is told
//! from its beginning until it is dropped, and two opens of one app at once, as two taps make, are
//! one under way until both are done.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::watch;
use waos_package::ProgramId;

/// Where an open under way is, in the words Android shows its user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenStage {
    /// Its code is being compiled again: missing for this engine, after an update of WaOS changed
    /// it, or no longer loading.
    Optimizing,
    /// Its program is loaded and its instance started.
    Starting,
}

/// The opens under way, by app, as their watchers hear them.
pub(super) struct Opens {
    stages: watch::Sender<BTreeMap<ProgramId, OpenStage>>,
    /// How many opens of each app are under way: two taps open one app twice, the second waiting
    /// for the first.
    counts: Mutex<HashMap<ProgramId, usize>>,
}

/// An open of one app, told to the watchers of the opens from its beginning until it is dropped.
pub(super) struct Open {
    opens: Arc<Opens>,
    id: ProgramId,
}

impl Opens {
    /// No open under way, the opens told to `stages` from now on.
    pub(super) fn new(stages: watch::Sender<BTreeMap<ProgramId, OpenStage>>) -> Opens {
        Opens {
            stages,
            counts: Mutex::default(),
        }
    }

    /// The opens under way now, each with its stage, and each change of them from now on.
    pub(super) fn stages(&self) -> watch::Receiver<BTreeMap<ProgramId, OpenStage>> {
        self.stages.subscribe()
    }

    /// The open of `id` under way is at `stage` from now on; its watchers hear only a change.
    /// Nothing where no open of `id` is under way, as when a compile goes on after its open was
    /// given up.
    fn set(&self, id: &ProgramId, stage: OpenStage) {
        let counts = self.counts.lock().unwrap_or_else(PoisonError::into_inner);
        if counts.contains_key(id) {
            self.stages
                .send_if_modified(|stages| stages.insert(id.clone(), stage) != Some(stage));
        }
    }
}

impl Open {
    /// An open of `id`, under way from now on.
    pub(super) fn begin(opens: &Arc<Opens>, id: &ProgramId) -> Open {
        let mut counts = opens.counts.lock().unwrap_or_else(PoisonError::into_inner);
        let count = counts.entry(id.clone()).or_default();
        *count += 1;
        if *count == 1 {
            opens.stages.send_modify(|stages| {
                stages.insert(id.clone(), OpenStage::Starting);
            });
        }
        Open {
            opens: Arc::clone(opens),
            id: id.clone(),
        }
    }

    /// The open is at `stage` from now on.
    pub(super) fn set(&self, stage: OpenStage) {
        self.opens.set(&self.id, stage);
    }

    /// What sets the open at `stage` when called, from any thread, as a load calls it as its
    /// compile begins.
    pub(super) fn set_later(&self, stage: OpenStage) -> impl FnOnce() + Send + 'static {
        let (opens, id) = (Arc::clone(&self.opens), self.id.clone());
        move || opens.set(&id, stage)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // Every change leaves the counts whole, so one that panicked leaves nothing half-changed.
        let mut counts = self
            .opens
            .counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(count) = counts.get_mut(&self.id) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            counts.remove(&self.id);
            self.opens.stages.send_modify(|stages| {
                stages.remove(&self.id);
            });
        }
    }
}
