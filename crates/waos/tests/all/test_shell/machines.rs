//! The machine picker as the tests have it: it picks what the test said, or cancels unless told,
//! at once, and keeps each pick asked of it, with the machines it showed.

use std::future::ready;
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use futures::channel::oneshot;
use waos::{AppInstance, MachineChoice, MachinePickedFuture, ShellMachinePicker};
use waos_headless::testing::Changes;

use super::announced_when_dropped;

/// What the picker answers, and what it was asked.
#[derive(Default)]
pub struct TestMachinePicker {
    /// The id of the machine the user picks; none to cancel.
    answer: Mutex<Option<String>>,
    /// When set, holds the next pick waiting until dropped or answered.
    hold: Mutex<bool>,
    held: Mutex<Vec<oneshot::Sender<Option<String>>>>,
    /// Each pick asked: the program's name and the machines shown.
    asked: Mutex<Vec<(String, Vec<MachineChoice>)>>,
    changes: Arc<Changes>,
}

impl TestMachinePicker {
    /// A picker asked nothing, announcing each pick, and each held pick's future dropped, to
    /// `changes`.
    pub fn announcing(changes: &Arc<Changes>) -> TestMachinePicker {
        TestMachinePicker {
            changes: Arc::clone(changes),
            ..TestMachinePicker::default()
        }
    }

    /// Picks the machine `id` from now on; none cancels.
    pub fn answer_with(&self, id: Option<&str>) {
        *self.answer.lock().unwrap() = id.map(str::to_owned);
    }

    /// Holds picks from now on until their futures are dropped.
    pub fn hold_picks(&self) {
        *self.hold.lock().unwrap() = true;
    }

    /// Whether the `index`th held pick's future was dropped without answering it.
    pub fn held_cancelled(&self, index: usize) -> bool {
        self.held
            .lock()
            .unwrap()
            .get(index)
            .is_some_and(oneshot::Sender::is_canceled)
    }

    /// Each pick asked, in order.
    pub fn asked(&self) -> Vec<(String, Vec<MachineChoice>)> {
        self.asked.lock().unwrap().clone()
    }
}

impl ShellMachinePicker for TestMachinePicker {
    fn pick(&self, program: &AppInstance, machines: Vec<MachineChoice>) -> MachinePickedFuture {
        self.asked
            .lock()
            .unwrap()
            .push((program.name().to_owned(), machines));
        self.changes.announce();
        if *self.hold.lock().unwrap() {
            let (tx, rx) = oneshot::channel();
            self.held.lock().unwrap().push(tx);
            return announced_when_dropped(&self.changes, rx.map(|picked| picked.unwrap_or(None)));
        }
        Box::pin(ready(self.answer.lock().unwrap().clone()))
    }
}
