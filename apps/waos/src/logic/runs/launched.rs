//! The runs `waos launch` starts, among WaOS's runs: a later launch of an id takes the place of
//! the one before, which is stopped first and ends `Replaced`, as does an install of a new build
//! of it (the design's decisions 29 and 70); and every program running, however it was started,
//! is the user's to read and drive through the dev socket, its input given through the program's
//! own mouse and the keyboard (decisions 60 and 70). Not on a phone, which `waos` reaches not yet.
//!
//! The invariant: a launch's mark is in the list under the same lock as its program, so a stop
//! for a later launch or install never finds the program without it, nor it without the program.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use waos::{ProgramEnd, ProgramView, RunningProgram};
use waos_dev_protocol::{Refusal, RunEnd, WindowId};
use waos_package::ProgramId;

use super::{Run, Runs};
use crate::logic::dev_refusal;

/// What a run `waos launch` started has in the list beside its program.
pub(super) struct Launched {
    /// Set before the program is stopped for a later launch or install of its id.
    replaced: Arc<AtomicBool>,
}

impl Launched {
    /// A later launch or install of its id takes its place: it ends `Replaced`.
    pub(super) fn replaced(&self) {
        self.replaced.store(true, Ordering::SeqCst);
    }
}

/// One run `waos launch` started, as whoever waits for its end holds it.
pub(crate) struct LaunchedRun {
    pub(crate) run: Run,
    replaced: Arc<AtomicBool>,
}

impl LaunchedRun {
    /// How the run ended, as an attach hears it: a stop for a later launch or install of the id
    /// is `Replaced`.
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

impl Runs {
    /// Starts a run `waos launch` asked for of the program `id`: the one running, if any, is
    /// stopped first, and ends `Replaced` where a launch started it; then `start` starts the
    /// program, and the run is in the list. An error from `start` leaves the id free. `start`
    /// waits for nothing: the program it starts is loaded already.
    pub(crate) async fn launch(
        &self,
        id: &ProgramId,
        start: impl FnOnce() -> Result<RunningProgram>,
    ) -> Result<LaunchedRun> {
        let _starting = super::Starting::of(self, id);
        let _turn = self.turn_of(id).await;
        self.end_the_one_before(id).await;
        let program = start()?;
        // The launch's mark joins the list under the same lock as its program.
        let mut running = self.listed();
        let run = self.list_in(&mut running, id, program);
        let replaced = Arc::new(AtomicBool::new(false));
        if let Some(listed) = running.get_mut(id) {
            listed.launched = Some(Launched {
                replaced: Arc::clone(&replaced),
            });
        }
        Ok(LaunchedRun { run, replaced })
    }

    /// Stops the program running as `id`, if any, for a new build of it installed in its place,
    /// and waits until it has ended: a launched run ends `Replaced`. Answers whether one ran.
    pub(crate) async fn stop_replaced(&self, id: &ProgramId) -> bool {
        let _turn = self.turn_of(id).await;
        let Some(current) = self.take_out(id) else {
            return false;
        };
        tracing::info!(%id, "a new build is installed: stopping its run");
        if let Some(launched) = &current.launched {
            launched.replaced();
        }
        current.program.stop();
        current.program.ended().await;
        true
    }

    /// The program `id` running now, however it was started, as the dev tools reach it.
    pub(crate) fn driven(&self, id: &ProgramId) -> Option<DrivenProgram> {
        Some(DrivenProgram {
            id: id.clone(),
            program: self.running_now(id)?,
        })
    }
}
