//! The machine picker as the tests have it: it picks what the test said, or cancels unless told,
//! at once, and keeps each pick asked of it, with the machines it showed.

use std::future::ready;
use std::sync::Mutex;

use waos::{MachineChoice, MachinePickedFuture, RunningProgram, ShellMachinePicker};

/// What the picker answers, and what it was asked.
#[derive(Default)]
pub struct TestMachinePicker {
    /// The id of the machine the user picks; none to cancel.
    answer: Mutex<Option<String>>,
    /// Each pick asked: the program's name and the machines shown.
    asked: Mutex<Vec<(String, Vec<MachineChoice>)>>,
}

impl TestMachinePicker {
    /// Picks the machine `id` from now on; none cancels.
    pub fn answer_with(&self, id: Option<&str>) {
        *self.answer.lock().unwrap() = id.map(str::to_owned);
    }

    /// Each pick asked, in order.
    pub fn asked(&self) -> Vec<(String, Vec<MachineChoice>)> {
        self.asked.lock().unwrap().clone()
    }
}

impl ShellMachinePicker for TestMachinePicker {
    fn pick(&self, program: &RunningProgram, machines: Vec<MachineChoice>) -> MachinePickedFuture {
        self.asked
            .lock()
            .unwrap()
            .push((program.name().to_owned(), machines));
        Box::pin(ready(self.answer.lock().unwrap().clone()))
    }
}
