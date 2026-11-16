//! Starting a program held for an inspector (decisions 18 and 19 of
//! `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`): the program the id names,
//! installed, by its component in the store, or else run in development, by the component its
//! last `cargo waos run` sent; loaded off the caller's thread, which compiles it unless the code
//! cache holds its code; then started held in place of the one running, which is stopped first,
//! as a later `cargo waos run` replaces a run, and kept among the runs as one the library opened,
//! with its settings and folders and this computer's capabilities, its output going to WaOS's log
//! and to an inspector that listens. Answered once it is linked, so the inspector can listen to it
//! from its first instruction.
//!
//! The invariant: a program is started only by the id of one installed or sent before, so an
//! inspector starts nothing WaOS does not already know as that program.

use std::path::PathBuf;
use std::sync::Arc;

use waos::HeldProgram;
use waos::inspection::bindings::wasdk::inspect::devices::StartError;
use waos_package::ProgramId;
use waos_store::Store;

use super::installed_runs::InstalledRuns;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use super::sent_components::SentComponents;

/// What starting a program held needs of the app.
pub(crate) struct HeldStarts {
    pub(crate) installed: InstalledRuns,
    pub(crate) store: Store,
    /// The components `cargo waos` sent, where a development run is started again from.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) sent: Arc<SentComponents>,
}

impl HeldStarts {
    /// Starts the program `id` held, in place of the one running, and answers it once linked.
    pub(crate) async fn start(&self, id: &str) -> Result<HeldProgram, StartError> {
        let id: ProgramId = id.parse().map_err(|_| StartError::Unknown)?;
        tracing::info!(%id, "an inspector asks to start it held");
        if let Some(installed) = self.store.program(&id) {
            return self.start_from(&id, installed).await;
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            // Held until the run is among the runs, as a run `cargo waos` sends holds it.
            let sent = self.sent.hold(&id).await;
            if sent.path().is_file() {
                return self.start_from(&id, sent.path().to_owned()).await;
            }
        }
        Err(StartError::Unknown)
    }

    /// Starts the program at `component` held as the program `id`, and answers it once linked.
    async fn start_from(
        &self,
        id: &ProgramId,
        component: PathBuf,
    ) -> Result<HeldProgram, StartError> {
        let mut held = self.load_and_start(id, component).await.map_err(|error| {
            tracing::info!(%id, "a held start refused: {error:#}");
            StartError::Failed(format!("{error:#}"))
        })?;
        held.linked().await;
        Ok(held)
    }

    /// Loads the program at `component` as the program `id` and starts it held in place of the
    /// one running.
    async fn load_and_start(
        &self,
        id: &ProgramId,
        component: PathBuf,
    ) -> anyhow::Result<HeldProgram> {
        let runs = &self.installed;
        let engine = Arc::clone(&runs.engine);
        let program = tokio::task::spawn_blocking(move || engine.load(&component))
            .await??
            .named(id.as_str());
        let environment = runs.environments.of(id)?;
        runs.places.set(id, runs.places.usual());
        let mut held = None;
        let run = runs
            .runs
            .start_in_place(id, || {
                let (running, hold) = runs.engine.run_held(&program, environment)?;
                held = Some(hold);
                Ok(running)
            })
            .await?;
        tracing::info!(%id, "started held for an inspector");
        runs.runs.take_out_once_ended(run);
        held.ok_or_else(|| anyhow::anyhow!("a run that started is held"))
    }
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use futures::StreamExt;
    use waos::TraceKind;
    use waos_local::LocalCapabilities;

    use super::*;
    use waos::inspection::InspectedPrograms;

    use crate::logic::environments::ProgramEnvironments;
    use crate::logic::inspected_programs::ProgramsOnThisComputer;
    use crate::logic::program_places::{ProgramPlace, ProgramPlaces};
    use crate::logic::runs::Runs;
    use crate::logic::test_engine::{self, NoWindows};

    #[tokio::test(flavor = "multi_thread")]
    async fn an_inspector_starts_a_development_run_held_in_place_of_the_one_running_and_ends_it() {
        let folder = tempfile::tempdir().unwrap();
        let engine = test_engine::engine(Arc::new(NoWindows), &folder.path().join("code"));
        let store =
            Store::open(&folder.path().join("store"), waos_store::SourceProxy::None).unwrap();
        let runs = Arc::new(Runs::default());
        let installed = InstalledRuns::new(
            Arc::new(engine),
            ProgramEnvironments::new(LocalCapabilities::new(), store.clone()),
            Arc::clone(&runs),
            ProgramPlaces::new(ProgramPlace::Windows),
        );
        let programs = ProgramsOnThisComputer::new(store.clone(), Arc::clone(&runs));
        let sent = Arc::new(SentComponents::new(folder.path().join("dev")));
        let id: ProgramId = "dev.example.forever".parse().unwrap();
        let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
        sent.hold(&id).await.write(&forever).unwrap();
        let starts = HeldStarts {
            installed,
            store,
            sent,
        };
        programs.serve_starts(starts, tokio::runtime::Handle::current());

        let held = programs.start_held("dev.example.forever").await.unwrap();
        let program = runs.running(&id).await.expect("among the runs");
        let mut output = program.trace(TraceKind::Output).expect("linked");
        drop(held);
        let first = output.next().await.expect("its first write");
        let json = serde_json::to_value(&first).unwrap();
        let bytes: Vec<u8> = serde_json::from_value(json["bytes"].clone()).unwrap();
        assert_eq!(String::from_utf8_lossy(&bytes), "count 1048576\n");

        let again = programs.start_held("dev.example.forever").await.unwrap();
        assert!(
            matches!(program.ended().await, waos::ProgramEnd::Stopped),
            "the run before is stopped first"
        );
        let started_again = runs.running(&id).await.expect("the new run");
        drop(again);
        assert!(
            matches!(
                programs.start_held("dev.example.never-sent").await,
                Err(StartError::Unknown)
            ),
            "an id neither installed nor sent"
        );
        programs.end("dev.example.forever").await;
        assert!(
            matches!(started_again.ended().await, waos::ProgramEnd::Stopped),
            "ended by an inspector"
        );
        assert!(runs.running(&id).await.is_none(), "and out of the runs");
    }
}
