//! Every program running, however it was installed or started, as the user reads and drives it
//! through the dev socket, its input given through the program's own mouse and the keyboard. Not on
//! a phone, which `waos` reaches not yet.
//!
//! The invariant: a program is reached by the id it runs as, the one the running apps list it under.

use waos::{ProgramView, RunningProgram};
use waos_dev_protocol::{Refusal, WindowId};
use waos_package::ProgramId;

use super::RunningApps;
use crate::logic::dev_refusal;

/// A program running in WaOS, however it was installed or started, as `waos`'s screenshots,
/// input, sizes and inspections read and drive it.
#[derive(Clone)]
pub(crate) struct DrivenProgram {
    pub(crate) id: ProgramId,
    pub(crate) program: RunningProgram,
}

impl DrivenProgram {
    /// The program's windows open now, each with the id the engine gave it as it opened (`ViewId`,
    /// minted from 1 and never reused within the run).
    pub(crate) fn windows(&self) -> Vec<(WindowId, ProgramView)> {
        self.program
            .views()
            .into_iter()
            .map(|view| (WindowId(view.id()), view))
            .collect()
    }

    /// The program's window `window` by the id it was given as it opened, or its first open
    /// window (lowest id) when `window` is `None`.
    pub(crate) fn window(
        &self,
        window: Option<WindowId>,
    ) -> Result<(WindowId, ProgramView), Refusal> {
        let windows = self.windows();
        let count = windows.len();
        match window {
            Some(window) => windows
                .into_iter()
                .find(|(each, _)| *each == window)
                .ok_or_else(|| dev_refusal::no_window(&self.id, window, count)),
            None => windows
                .into_iter()
                .min_by_key(|(each, _)| *each)
                .ok_or_else(|| dev_refusal::no_window(&self.id, WindowId(1), 0)),
        }
    }
}

impl RunningApps {
    /// The program `id` running now, however it was started, as the dev tools reach it.
    pub(crate) fn driven(&self, id: &ProgramId) -> Option<DrivenProgram> {
        Some(DrivenProgram {
            id: id.clone(),
            program: self.running(id)?,
        })
    }
}
