//! The runs `cargo waos` sends, among WaOS's runs: a later run of an id takes the place of the one
//! before, which is stopped first and ends `Replaced` (the design's decision 29), and each is its
//! own to read and drive, with a mouse of its own over the program's windows (decision 60). Not on
//! a phone, which `cargo waos` reaches not yet.
//!
//! The invariant: a sent run's mark and mouse are in the list under the same lock as its program,
//! so a screenshot or an input never finds the program without them, nor them without it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use waos::{ProgramEnd, ProgramView, RunningProgram};
use waos_dev_protocol::{Refusal, RunEnd, WindowId};
use waos_package::ProgramId;

use super::{Run, Runs};
use crate::logic::dev_input::DevMouse;
use crate::logic::dev_refusal;

/// What a run `cargo waos` sent has in the list beside its program.
pub(super) struct Sent {
    /// Set before the program is stopped for a later run of its id.
    replaced: Arc<AtomicBool>,
    /// The mouse `cargo waos`'s input moves over the program's windows.
    mouse: Arc<DevMouse>,
}

impl Sent {
    /// A later run of its id takes its place: it ends `Replaced`.
    pub(super) fn replaced(&self) {
        self.replaced.store(true, Ordering::SeqCst);
    }
}

/// One run `cargo waos` sent, as its connection holds it.
pub(crate) struct SentRun {
    pub(crate) run: Run,
    replaced: Arc<AtomicBool>,
}

impl SentRun {
    /// How the run ended, as its connection hears it: a stop for a later run of the id is
    /// `Replaced`.
    pub(crate) fn end(&self, end: &ProgramEnd) -> RunEnd {
        match end {
            ProgramEnd::Returned(Ok(())) => RunEnd::Returned { success: true },
            ProgramEnd::Returned(Err(())) => RunEnd::Returned { success: false },
            ProgramEnd::Failed(error) => RunEnd::Failed(format!("{error:#}")),
            ProgramEnd::Stopped if self.replaced.load(Ordering::SeqCst) => RunEnd::Replaced,
            ProgramEnd::Stopped => RunEnd::Stopped,
        }
    }
}

/// A program a run of `cargo waos run` runs, which `cargo waos`'s screenshots and input read and
/// drive: the program, and the mouse that input moves over its windows.
#[derive(Clone)]
pub(crate) struct DevRun {
    pub(crate) id: ProgramId,
    pub(crate) program: RunningProgram,
    pub(crate) mouse: Arc<DevMouse>,
}

impl DevRun {
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

impl Runs {
    /// Starts a run `cargo waos` sent of the program `id`: the one running, if any, is stopped
    /// first, and ends `Replaced` where `cargo waos` sent it; then `start` starts the program, and
    /// the run is in the list, with a mouse of its own for `cargo waos`'s input. An error from
    /// `start` leaves the id free. `start` waits for nothing: the program it starts is loaded
    /// already.
    pub(crate) async fn start(
        &self,
        id: &ProgramId,
        start: impl FnOnce() -> Result<RunningProgram>,
    ) -> Result<SentRun> {
        let _turn = self.turn_of(id).await;
        self.end_the_one_before(id).await;
        let program = start()?;
        // The run's mark and mouse join the list under the same lock as its program.
        let mut running = self.listed();
        let run = self.list_in(&mut running, id, program);
        let replaced = Arc::new(AtomicBool::new(false));
        if let Some(listed) = running.get_mut(id) {
            listed.sent = Some(Sent {
                replaced: Arc::clone(&replaced),
                mouse: Arc::default(),
            });
        }
        Ok(SentRun { run, replaced })
    }

    /// The program `id` running now, if `cargo waos run` sent it; none for one the library
    /// opened.
    pub(crate) async fn dev_run(&self, id: &ProgramId) -> Option<DevRun> {
        let running = self.listed();
        let current = running.get(id)?;
        Some(DevRun {
            id: id.clone(),
            program: current.program.clone(),
            mouse: Arc::clone(&current.sent.as_ref()?.mouse),
        })
    }
}
