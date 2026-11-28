//! The instances running, one per app at most, by the app's id, and each app's turn: one open or
//! stop of an app at a time, from the moment it looks for the instance before to the moment the
//! new one is in the list, so two opens of one app arriving together cannot both start it, and an
//! id never names two instances. The list's own lock is held only to read or change the list, never
//! while an instance starts or ends, so an instance slow to end holds back no screenshot, input,
//! open or quit, nor another app's open; the ids running, which [`InstanceList::ids`] tells, change
//! with the list under that lock. Each instance knows whether a later one took its place, so its
//! end is told as `Replaced`.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::{OwnedMutexGuard, watch};
use waos::{AppInstance, ProgramEnd};
use waos_dev_protocol::RunEnd;
use waos_package::ProgramId;

/// The instances running, by their app's id.
#[derive(Default)]
pub(super) struct InstanceList {
    /// The instances in the list, held only to read or change it.
    running: Mutex<HashMap<ProgramId, Listed>>,
    /// Each app's turn, one open or stop of the app at a time.
    turns: Mutex<HashMap<ProgramId, Arc<tokio::sync::Mutex<()>>>>,
    /// Tells one instance of an app from the next.
    serials: AtomicU64,
    /// The ids in the list, for whoever shows which apps run.
    ids: watch::Sender<BTreeSet<ProgramId>>,
}

/// One instance in the list.
struct Listed {
    serial: u64,
    instance: AppInstance,
    /// Set before the instance is stopped for a later one of its app, or a new build of it.
    replaced: Arc<AtomicBool>,
}

/// One instance, as whoever opened it holds it.
pub(super) struct Run {
    pub(super) app: ProgramId,
    serial: u64,
    pub(super) instance: AppInstance,
    replaced: Arc<AtomicBool>,
}

impl Run {
    /// How the instance ended, as an attach hears it: a stop for a later instance of its app, or a
    /// new build of it, is `Replaced`.
    pub(super) fn end(&self, end: &ProgramEnd) -> RunEnd {
        match end {
            ProgramEnd::Returned(Ok(())) => RunEnd::Returned { success: true },
            ProgramEnd::Returned(Err(())) => RunEnd::Returned { success: false },
            ProgramEnd::Failed(error) => RunEnd::Failed(format!("{error:#}")),
            ProgramEnd::Stopped if self.replaced.load(Ordering::SeqCst) => RunEnd::Replaced,
            ProgramEnd::Stopped => RunEnd::Stopped,
        }
    }
}

impl InstanceList {
    /// The turn of the app `id`, once no other open or stop of it holds it.
    pub(super) async fn turn_of(&self, id: &ProgramId) -> OwnedMutexGuard<()> {
        let turn = {
            let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(turns.entry(id.clone()).or_default())
        };
        turn.lock_owned().await
    }

    /// Stops the instance of `id`, if any, once an open or stop of the app under way is done, and
    /// waits until it has ended: it is out of the list first, so nothing finds it running
    /// meanwhile, and ends `Replaced` where `replaced` says, as for a new build of it installed in
    /// its place. Answers whether one ran.
    pub(super) async fn stop(&self, id: &ProgramId, replaced: bool) -> bool {
        let _turn = self.turn_of(id).await;
        let ran = self.end(id, replaced).await;
        if ran {
            tracing::info!(%id, replaced, "its instance is stopped");
        }
        ran
    }

    /// Stops the instance of `id`, if any, and waits until it has ended, out of the list first;
    /// it ends `Replaced` where `replaced` says. Answers whether one ran. The caller holds the
    /// app's turn.
    pub(super) async fn end(&self, id: &ProgramId, replaced: bool) -> bool {
        let Some(before) = self.take_out(id) else {
            return false;
        };
        before.replaced.store(replaced, Ordering::SeqCst);
        before.instance.stop();
        before.instance.ended().await;
        true
    }

    /// Takes the instance of `id` out of the list, so nothing finds it running while it ends.
    fn take_out(&self, id: &ProgramId) -> Option<Listed> {
        let mut running = self.listed();
        let current = running.remove(id)?;
        self.ids.send_replace(running.keys().cloned().collect());
        Some(current)
    }

    /// Puts `instance` in the list as the instance of `id`. The caller holds the app's turn.
    pub(super) fn list(&self, id: &ProgramId, instance: AppInstance) -> Run {
        let mut running = self.listed();
        let serial = self.serials.fetch_add(1, Ordering::SeqCst);
        let replaced = Arc::new(AtomicBool::new(false));
        running.insert(
            id.clone(),
            Listed {
                serial,
                instance: instance.clone(),
                replaced: Arc::clone(&replaced),
            },
        );
        self.ids.send_replace(running.keys().cloned().collect());
        Run {
            app: id.clone(),
            serial,
            instance,
            replaced,
        }
    }

    /// Takes `run` out of the list once it has ended, unless a later instance of its app took its
    /// place already.
    pub(super) fn ended(&self, run: &Run) {
        let mut running = self.listed();
        if running
            .get(&run.app)
            .is_some_and(|current| current.serial == run.serial)
        {
            running.remove(&run.app);
            self.ids.send_replace(running.keys().cloned().collect());
        }
    }

    /// The ids running now, and each change of them from now on.
    pub(super) fn ids(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.ids.subscribe()
    }

    /// The instance of `id` running now, if one is.
    pub(super) fn instance(&self, id: &ProgramId) -> Option<AppInstance> {
        self.listed()
            .get(id)
            .map(|current| current.instance.clone())
    }

    /// The instances running now, in no order.
    pub(super) fn instances(&self) -> Vec<AppInstance> {
        (self.listed().values())
            .map(|current| current.instance.clone())
            .collect()
    }

    /// The list, for a moment: never held across an await.
    fn listed(&self) -> MutexGuard<'_, HashMap<ProgramId, Listed>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use waos::ProgramEnvironment;

    use super::*;
    use crate::logic::test_engine::{self, NoWindows};

    #[tokio::test(flavor = "multi_thread")]
    async fn the_list_is_read_and_another_app_opened_while_one_app_holds_its_turn() {
        let engine = test_engine::engine(Arc::new(NoWindows));
        let forever = engine
            .load(
                Path::new(waos_test_programs::FOREVER),
                &waos_test_programs::code_of(waos_test_programs::FOREVER),
            )
            .unwrap()
            .named("dev.example.other");
        let list = InstanceList::default();
        let slow: ProgramId = "dev.example.slow".parse().unwrap();
        let other: ProgramId = "dev.example.other".parse().unwrap();
        let _slow_turn = list.turn_of(&slow).await;

        let at_once = Duration::from_secs(1);
        assert!(list.ids().borrow().is_empty(), "the list is read meanwhile");
        let other_turn = tokio::time::timeout(at_once, list.turn_of(&other))
            .await
            .expect("and another app's turn comes meanwhile");
        let instance = engine.run(&forever, ProgramEnvironment::default()).unwrap();
        let run = list.list(&other, instance);
        drop(other_turn);
        assert!(list.instance(&other).as_ref() == Some(&run.instance));
        assert!(list.instance(&slow).is_none());

        assert!(list.stop(&other, false).await, "it ran");
        assert!(
            list.instance(&other).is_none(),
            "out of the list once stopped"
        );
    }
}
