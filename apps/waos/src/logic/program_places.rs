//! Where each program's views go, by where it was opened: a program in a tab of the deck has its
//! tab's screen reserved for its first request, and each later request goes to the deck on the main
//! thread, which opens another tab for it, each tab a window (`state::deck`); any other program's
//! requests go to the windows, WaOS's own (`placement`) or, on macOS, its app's. A program opened
//! without saying where, as `waos` runs one, goes where this WaOS puts programs unless told
//! otherwise.
//!
//! The invariant: a program's place is set before its instance starts (`app_instances`), so its
//! first request already goes where its instance is shown; and a request taken for a tab is on its way
//! to the main thread, which answers it once, so every `Ok` is followed by one answer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use waos::bindings::wasdk::platform::views::ViewError;
use waos::{AppInstance, ViewPlacement, ViewRequest};
use waos_package::ProgramId;

/// Where a program's views go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProgramPlace {
    /// In tabs of the deck: its first view the tab it was opened in, each later one a tab of its
    /// own.
    Tab,
    /// In windows, made at its request: a desktop's, or a task each on Android.
    Windows,
}

/// Where each program opened goes, by its id, and where any other does.
#[derive(Clone)]
pub(crate) struct ProgramPlaces {
    usual: ProgramPlace,
    opened: Arc<Mutex<HashMap<String, ProgramPlace>>>,
}

impl ProgramPlaces {
    /// Places in which a program opened without saying where goes to `usual`.
    pub(crate) fn new(usual: ProgramPlace) -> ProgramPlaces {
        ProgramPlaces {
            usual,
            opened: Arc::default(),
        }
    }

    /// Where a program opened without saying where goes.
    pub(crate) fn usual(&self) -> ProgramPlace {
        self.usual
    }

    /// The program `id` goes to `place` from now on.
    pub(crate) fn set(&self, id: &ProgramId, place: ProgramPlace) {
        self.opened
            .lock()
            .expect("no thread panics holding the places")
            .insert(id.as_str().to_owned(), place);
    }

    /// Where the program `id` goes.
    pub(crate) fn place_of(&self, id: &ProgramId) -> ProgramPlace {
        self.of(id.as_str())
    }

    /// Where the program running as `name` goes.
    fn of(&self, name: &str) -> ProgramPlace {
        let opened = self
            .opened
            .lock()
            .expect("no thread panics holding the places");
        opened.get(name).copied().unwrap_or(self.usual)
    }
}

/// One request of a program in the deck, for a tab of its own, on its way to the main thread.
pub(crate) struct TabRequest {
    pub(crate) program: AppInstance,
    pub(crate) request: ViewRequest,
}

/// The placement in the deck's tabs, as the engine reaches it: every request goes to the main
/// thread.
#[derive(Clone)]
pub(crate) struct TabsOnRequest(UnboundedSender<TabRequest>);

impl TabsOnRequest {
    /// The placement, and where its requests arrive on the main thread; a WaOS with no deck drops
    /// the receiver, and a request is refused.
    pub(crate) fn new() -> (TabsOnRequest, UnboundedReceiver<TabRequest>) {
        let (requests, arriving) = mpsc::unbounded();
        (TabsOnRequest(requests), arriving)
    }
}

impl ViewPlacement for TabsOnRequest {
    fn view_requested(&self, program: &AppInstance, request: ViewRequest) -> Result<(), ViewError> {
        let program = program.clone();
        let name = program.name().to_owned();
        match self.0.unbounded_send(TabRequest { program, request }) {
            Ok(()) => {
                tracing::info!(program = name, "another tab was asked for");
                Ok(())
            }
            Err(_) => Err(ViewError::Unsupported),
        }
    }
}

/// The engine's placement: each program's request handed to `tabs` or to `windows`, by where it
/// was opened.
pub(crate) struct PlacedWhereOpened {
    pub(crate) places: ProgramPlaces,
    pub(crate) tabs: TabsOnRequest,
    pub(crate) windows: Arc<dyn ViewPlacement>,
}

impl ViewPlacement for PlacedWhereOpened {
    fn view_requested(&self, program: &AppInstance, request: ViewRequest) -> Result<(), ViewError> {
        match self.places.of(program.name()) {
            ProgramPlace::Tab => self.tabs.view_requested(program, request),
            ProgramPlace::Windows => self.windows.view_requested(program, request),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use futures::StreamExt;
    use waos::{ProgramEnd, ProgramEnvironment, WaOS};

    use super::*;
    use crate::logic::placement::WindowsOnRequest;
    use crate::logic::placement::tests::packages_in;
    use crate::logic::program_faces::ProgramFaces;
    use crate::logic::test_engine::{self, WITHIN};

    /// Survey's id, as the tests name the program the library opens.
    const SURVEY: &str = "dev.example.survey";

    /// Runs survey, which asks for a view as it starts, as `SURVEY`, on `engine`.
    fn run_survey(engine: &WaOS) -> AppInstance {
        let survey = engine
            .load(
                Path::new(waos_test_programs::SURVEY),
                &waos_test_programs::code_of(waos_test_programs::SURVEY),
            )
            .expect("survey loads")
            .named(SURVEY);

        engine
            .run(&survey, ProgramEnvironment::default())
            .expect("survey runs")
    }

    /// A request of a program in a tab that its tab's screen did not take goes to the deck.
    #[tokio::test]
    async fn a_request_of_a_program_in_a_tab_goes_to_the_deck() {
        let folder = tempfile::tempdir().unwrap();
        let (windows, _windows_asked) =
            WindowsOnRequest::new(ProgramFaces::new(packages_in(folder.path()), None));
        let (tabs, mut tabs_asked) = TabsOnRequest::new();
        let places = ProgramPlaces::new(ProgramPlace::Windows);
        places.set(&SURVEY.parse().unwrap(), ProgramPlace::Tab);
        let placement = PlacedWhereOpened {
            places,
            tabs,
            windows: Arc::new(windows),
        };
        let engine = test_engine::engine(Arc::new(placement));
        let running = run_survey(&engine);
        let arrived = tokio::time::timeout(WITHIN, tabs_asked.next())
            .await
            .expect("survey asks in time")
            .expect("a request for a tab");
        assert!(arrived.program == running);
        running.view_request_failed(arrived.request.id, ViewError::Unsupported);
        let end = tokio::time::timeout(WITHIN, running.ended()).await.unwrap();
        assert!(matches!(end, ProgramEnd::Returned(Err(()))), "{end:?}");
    }

    /// A WaOS with no deck refuses a request for a tab at once.
    #[tokio::test]
    async fn with_no_deck_a_request_for_a_tab_is_refused_at_once() {
        let folder = tempfile::tempdir().unwrap();
        let (windows, _windows_asked) =
            WindowsOnRequest::new(ProgramFaces::new(packages_in(folder.path()), None));
        let (tabs, no_deck) = TabsOnRequest::new();
        drop(no_deck);
        let places = ProgramPlaces::new(ProgramPlace::Tab);
        let placement = PlacedWhereOpened {
            places,
            tabs,
            windows: Arc::new(windows),
        };
        let engine = test_engine::engine(Arc::new(placement));
        let running = run_survey(&engine);
        let end = tokio::time::timeout(WITHIN, running.ended()).await.unwrap();
        assert!(matches!(end, ProgramEnd::Returned(Err(()))), "{end:?}");
    }

    #[tokio::test]
    async fn a_program_placed_nowhere_goes_where_programs_usually_go() {
        let folder = tempfile::tempdir().unwrap();
        let (windows, mut arriving) =
            WindowsOnRequest::new(ProgramFaces::new(packages_in(folder.path()), None));
        let placement = PlacedWhereOpened {
            places: ProgramPlaces::new(ProgramPlace::Windows),
            tabs: TabsOnRequest::new().0,
            windows: Arc::new(windows),
        };
        let engine = test_engine::engine(Arc::new(placement));
        let running = run_survey(&engine);
        let arrived = tokio::time::timeout(WITHIN, arriving.next())
            .await
            .expect("survey asks in time")
            .expect("a request for a window");
        running.view_request_failed(arrived.request.id, ViewError::Unsupported);
    }
}
