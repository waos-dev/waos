//! Where each program's views go, by where it was opened: a program in a tab of the deck has the
//! view its tab gave it as it started, and every request it makes is refused at once, as the views
//! WIT says of a phone (`state::deck`); any other program's requests go to the windows, WaOS's own
//! (`placement`) or, on macOS, its app's. A program opened without saying where, as `waos`
//! runs one, goes where this WaOS puts programs unless told otherwise.
//!
//! The invariant: a program's place is set before its run starts (`InstalledRuns::open`), so its
//! first request already goes where its run is shown.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use waos::bindings::wasdk::platform::views::ViewError;
use waos::{RunningProgram, ViewPlacement, ViewRequest};
use waos_package::ProgramId;

/// Where a program's views go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProgramPlace {
    /// In its tab of the deck, which gave it its view.
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

/// The engine's placement: each program's request refused in its tab, or handed to `windows`.
pub(crate) struct PlacedWhereOpened {
    pub(crate) places: ProgramPlaces,
    pub(crate) windows: Arc<dyn ViewPlacement>,
}

impl ViewPlacement for PlacedWhereOpened {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        match self.places.of(program.name()) {
            ProgramPlace::Tab => {
                tracing::info!(
                    program = program.name(),
                    "a view was asked for in a tab: refused"
                );
                Err(ViewError::Unsupported)
            }
            ProgramPlace::Windows => self.windows.view_requested(program, request),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use futures::StreamExt;
    use waos::{ProgramEnd, ProgramEnvironment, WaOS};
    use waos_test_programs::CODE_CACHE;

    use super::*;
    use crate::logic::placement::WindowsOnRequest;
    use crate::logic::placement::tests::store_in;
    use crate::logic::program_faces::ProgramFaces;
    use crate::logic::test_engine;

    /// How long a test waits for a program to do something.
    const WITHIN: Duration = Duration::from_secs(60);

    /// Survey's id, as the tests name the program the library opens.
    const SURVEY: &str = "dev.example.survey";

    /// Runs survey, which asks for a view as it starts, as `SURVEY`, on `engine`.
    fn run_survey(engine: &WaOS) -> RunningProgram {
        let survey = engine
            .load(Path::new(waos_test_programs::SURVEY))
            .expect("survey loads")
            .named(SURVEY);

        engine
            .run(&survey, ProgramEnvironment::default())
            .expect("survey runs")
    }

    /// Survey asks for a view as it starts, and ends with a failure when it gets none.
    #[tokio::test]
    async fn a_program_in_a_tab_is_refused_at_once() {
        let folder = tempfile::tempdir().unwrap();
        let (windows, _arriving) =
            WindowsOnRequest::new(ProgramFaces::new(store_in(folder.path()), None));
        let places = ProgramPlaces::new(ProgramPlace::Windows);
        places.set(&SURVEY.parse().unwrap(), ProgramPlace::Tab);
        let placement = PlacedWhereOpened {
            places,
            windows: Arc::new(windows),
        };
        let engine = test_engine::engine(Arc::new(placement), Path::new(CODE_CACHE));
        let running = run_survey(&engine);
        let end = tokio::time::timeout(WITHIN, running.ended()).await.unwrap();
        assert!(matches!(end, ProgramEnd::Returned(Err(()))), "{end:?}");
    }

    #[tokio::test]
    async fn a_program_placed_nowhere_goes_where_programs_usually_go() {
        let folder = tempfile::tempdir().unwrap();
        let (windows, mut arriving) =
            WindowsOnRequest::new(ProgramFaces::new(store_in(folder.path()), None));
        let placement = PlacedWhereOpened {
            places: ProgramPlaces::new(ProgramPlace::Windows),
            windows: Arc::new(windows),
        };
        let engine = test_engine::engine(Arc::new(placement), Path::new(CODE_CACHE));
        let running = run_survey(&engine);
        let arrived = tokio::time::timeout(WITHIN, arriving.next())
            .await
            .expect("survey asks in time")
            .expect("a request for a window");
        running.view_request_failed(arrived.request.id, ViewError::Unsupported);
    }
}
