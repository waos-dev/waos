//! Where the programs' view requests go when they go to windows of WaOS's own, on a desktop and
//! on Android, where each is a task: each to the main thread, with the program's name and icon
//! as the store has them, with its identity where it has one (`program_faces`), which asks Waft for
//! a window for it
//! (`waos-program-window`'s `ViewWindows`). The engine asks from the program's thread and wants
//! its answer at once, so a request taken is answered later, there, with the view or the failure.
//!
//! The invariant: a request is taken only once it is on its way to the main thread, so every
//! `Ok` is followed by exactly one answer while the app runs; once the main thread has stopped
//! listening, every request is refused.

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use waos::bindings::wasdk::platform::views::ViewError;
use waos::{RunningProgram, ViewPlacement, ViewRequest};
use waos_program_window::WindowRequest;

use super::program_faces::ProgramFaces;

/// One program's request for a window of WaOS's own, on its way to the main thread.
pub(crate) type ProgramWindowRequest = WindowRequest<RunningProgram>;

/// The placement in WaOS's own windows, as the engine reaches it: every request goes to the main
/// thread.
pub(crate) struct WindowsOnRequest {
    requests: UnboundedSender<ProgramWindowRequest>,
    /// The programs' names, icons and identities.
    faces: ProgramFaces,
}

impl WindowsOnRequest {
    /// The placement, showing the programs as `faces` says, and where its requests arrive on the
    /// main thread.
    pub(crate) fn new(
        faces: ProgramFaces,
    ) -> (WindowsOnRequest, UnboundedReceiver<ProgramWindowRequest>) {
        let (requests, arriving) = mpsc::unbounded();
        (WindowsOnRequest { requests, faces }, arriving)
    }
}

impl ViewPlacement for WindowsOnRequest {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        let title = request.config.title.clone();
        let sent = self.requests.unbounded_send(WindowRequest {
            program: program.clone(),
            request,
            face: self.faces.face(program.name()),
        });
        match sent {
            Ok(()) => {
                tracing::info!(program = program.name(), title, "a window was asked for");
                Ok(())
            }
            Err(_) => Err(ViewError::Failed("WaOS is quitting".to_owned())),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use futures::StreamExt;
    use waos::{ProgramEnd, ProgramEnvironment, WaOS};
    use waos_store::Store;
    use waos_test_programs::CODE_CACHE;

    use super::*;
    use crate::logic::test_engine;

    /// How long a test waits for a program to do something.
    const WITHIN: Duration = Duration::from_secs(60);

    /// A store with nothing installed, in `folder`.
    pub(crate) fn store_in(folder: &Path) -> Store {
        Store::open(&folder.join("store"), waos_store::SourceProxy::None).expect("a store")
    }

    /// Runs survey, which asks for a view as it starts, on `engine`.
    fn run_survey(engine: &WaOS) -> RunningProgram {
        let survey = engine
            .load(Path::new(waos_test_programs::SURVEY))
            .expect("survey loads");
        let (running, _given) = engine
            .run(&survey, ProgramEnvironment::default(), Vec::new())
            .expect("survey runs");
        running
    }

    #[tokio::test]
    async fn a_request_reaches_the_main_thread_with_its_program_and_waits_for_its_answer() {
        let folder = tempfile::tempdir().unwrap();
        let (placement, mut arriving) =
            WindowsOnRequest::new(ProgramFaces::new(store_in(folder.path()), None));
        let engine = test_engine::engine(Arc::new(placement), Path::new(CODE_CACHE));
        let running = run_survey(&engine);
        let arrived = tokio::time::timeout(WITHIN, arriving.next())
            .await
            .expect("survey asks in time")
            .expect("a request");
        assert_eq!(
            arrived.program.name(),
            running.name(),
            "the one program running"
        );
        assert_eq!(arrived.request.config.title, "Survey");
        assert_eq!(
            arrived.face.name,
            running.name(),
            "what it runs as, installed nowhere"
        );
        assert!(arrived.face.icon.is_none());
        running.view_request_failed(arrived.request.id, ViewError::Unsupported);
        let end = tokio::time::timeout(WITHIN, running.ended()).await.unwrap();
        assert!(matches!(end, ProgramEnd::Returned(Err(()))), "{end:?}");
    }

    #[tokio::test]
    async fn a_request_is_refused_once_the_main_thread_stopped_listening() {
        let folder = tempfile::tempdir().unwrap();
        let (placement, arriving) =
            WindowsOnRequest::new(ProgramFaces::new(store_in(folder.path()), None));
        drop(arriving);
        let engine = test_engine::engine(Arc::new(placement), Path::new(CODE_CACHE));
        let running = run_survey(&engine);
        let end = tokio::time::timeout(WITHIN, running.ended()).await.unwrap();
        assert!(matches!(end, ProgramEnd::Returned(Err(()))), "{end:?}");
    }
}
