//! Starting a program held for an inspector, as `wasdk:inspect` asks: the
//! installed program the id names, a development install's among them, started held
//! (`InstalledRuns::start_held`), and answered once it is linked, so the inspector can listen to it
//! from its first instruction.
//!
//! The invariant: a program is started only by the id of one installed, so an inspector starts
//! nothing WaOS does not already know as that program.

use waos::HeldProgram;
use waos::inspection::bindings::wasdk::inspect::devices::StartError;
use waos_package::ProgramId;
use waos_store::Store;

use super::installed_runs::InstalledRuns;

/// What starting a program held needs of the app.
pub(crate) struct HeldStarts {
    pub(crate) installed: InstalledRuns,
    pub(crate) store: Store,
}

impl HeldStarts {
    /// Starts the program `id` held, in place of the one running, and answers it once linked.
    pub(crate) async fn start(&self, id: &str) -> Result<HeldProgram, StartError> {
        let id: ProgramId = id.parse().map_err(|_| StartError::Unknown)?;
        tracing::info!(%id, "an inspector asks to start it held");
        if self.store.installed_program(&id).is_none() {
            return Err(StartError::Unknown);
        }
        let mut held = self.installed.start_held(&id).await.map_err(|error| {
            tracing::info!(%id, "a held start refused: {error:#}");
            StartError::Failed(format!("{error:#}"))
        })?;
        held.linked().await;
        Ok(held)
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use std::sync::Arc;

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
    async fn an_inspector_starts_a_development_install_held_in_place_of_the_one_running_and_ends_it()
     {
        let folder = tempfile::tempdir().unwrap();
        let engine = Arc::new(test_engine::engine(Arc::new(NoWindows)));
        let store =
            Store::open(&folder.path().join("store"), waos_store::SourceProxy::None).unwrap();
        let runs = Arc::new(Runs::default());
        let installed = InstalledRuns::new(
            Arc::clone(&engine),
            store.clone(),
            ProgramEnvironments::new(LocalCapabilities::new(), store.clone()),
            Arc::clone(&runs),
            ProgramPlaces::new(ProgramPlace::Windows),
        );
        let programs = ProgramsOnThisComputer::new(store.clone(), Arc::clone(&runs));
        let manifest = waos_package::Manifest::new(
            "dev.example.forever".parse().unwrap(),
            "Forever",
            "0.1.0".parse().unwrap(),
        )
        .unwrap();
        let forever = std::fs::read(waos_test_programs::FOREVER).unwrap();
        let build = waos_store::DevelopmentBuild {
            manifest: &manifest,
            component: &forever,
            icon: None,
        };
        crate::logic::test_installs::install_development(
            &store,
            &engine,
            &build,
            waos_test_programs::FOREVER,
        );
        let id: ProgramId = "dev.example.forever.development".parse().unwrap();
        let starts = HeldStarts { installed, store };
        programs.serve_starts(starts, tokio::runtime::Handle::current());

        let held = programs.start_held(id.as_str()).await.unwrap();
        let program = runs.running(&id).await.expect("among the runs");
        let mut output = program.trace(TraceKind::Output).expect("linked");
        drop(held);
        let first = output.next().await.expect("its first write");
        let json = serde_json::to_value(&first).unwrap();
        let bytes: Vec<u8> = serde_json::from_value(json["bytes"].clone()).unwrap();
        assert_eq!(String::from_utf8_lossy(&bytes), "count 1048576\n");

        let again = programs.start_held(id.as_str()).await.unwrap();
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
            "an id not installed"
        );
        programs.end(id.as_str()).await;
        assert!(
            matches!(started_again.ended().await, waos::ProgramEnd::Stopped),
            "ended by an inspector"
        );
        assert!(runs.running(&id).await.is_none(), "and out of the runs");
    }
}
