//! Starting a program held for an inspector, as `wasdk:inspect` asks: the
//! installed program the id names, a development install's among them, started held in place of
//! the one running (`app_starter`), and answered once it is linked, so the inspector can listen to
//! it from its first instruction.
//!
//! The invariant: a program is started only by the id of one installed, so an inspector starts
//! nothing WaOS does not already know as that program.

use std::sync::Arc;

use anyhow::Context as _;
use waos::HeldProgram;
use waos::inspection::bindings::wasdk::inspect::devices::StartError;
use waos_package::ProgramId;
use waos_services::packages::PackageManager;

use super::app_starter::{AppStarter, Opening, StartRequest};
use super::running_apps::IfRunning;

/// What starting a program held needs of the app.
pub(crate) struct HeldStarts {
    pub(crate) starter: Arc<AppStarter>,
    pub(crate) packages: Arc<PackageManager>,
}

impl HeldStarts {
    /// Starts the program `id` held, in place of the one running, and answers it once linked.
    pub(crate) async fn start(&self, id: &str) -> Result<HeldProgram, StartError> {
        let id: ProgramId = id.parse().map_err(|_| StartError::Unknown)?;
        tracing::info!(%id, "an inspector asks to start it held");
        if self.packages.installed_program(&id).is_none() {
            return Err(StartError::Unknown);
        }
        let request = StartRequest {
            if_running: IfRunning::Replace,
            held: true,
            ..StartRequest::open(id.clone(), Opening::ByTheDeveloper)
        };
        let started = self.starter.start(request).await;
        let held = started.and_then(|answer| answer.held().context("a held start holds the app"));
        let mut held = held.map_err(|error| {
            tracing::info!(%id, "a held start refused: {error:#}");
            StartError::Failed(format!("{error:#}"))
        })?;
        held.linked().await;
        Ok(held)
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use futures::StreamExt;
    use waos::TraceKind;

    use super::*;
    use waos::inspection::InspectedPrograms;

    use crate::logic::inspected_programs::ProgramsOnThisComputer;
    use crate::logic::startup::{self, Setup};
    use crate::logic::test_engine::{self, NoWindows};
    use crate::logic::test_installs;

    #[tokio::test(flavor = "multi_thread")]
    async fn an_inspector_starts_a_development_install_held_in_place_of_the_one_running_and_ends_it()
     {
        let folder = tempfile::tempdir().unwrap();
        let setup = Setup::for_tests(folder.path(), tokio::runtime::Handle::current());
        let started = startup::tests::start_asking_nobody(setup, |_before| {
            let shell = test_engine::shell(Arc::new(NoWindows));
            (shell, test_engine::no_screens())
        });
        let (runs, packages) = (Arc::clone(&started.running), Arc::clone(&started.packages));
        // What an inspector sees, as the engine is handed it.
        let programs = ProgramsOnThisComputer::new(
            started.storage,
            started.permissions,
            Arc::clone(&packages),
            Arc::clone(&runs),
        );
        let manifest = waos_package::Manifest::new(
            "dev.example.forever".parse().unwrap(),
            "Forever",
            "0.1.0".parse().unwrap(),
        )
        .unwrap();
        test_installs::install_development(&packages, manifest, waos_test_programs::FOREVER);
        let id: ProgramId = "dev.example.forever.development".parse().unwrap();
        let starts = HeldStarts {
            starter: started.starter,
            packages,
        };
        programs.serve_starts(starts, tokio::runtime::Handle::current());

        let held = programs.start_held(id.as_str()).await.unwrap();
        let program = runs.running(&id).expect("among the runs");
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
        let started_again = runs.running(&id).expect("the new run");
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
        assert!(runs.running(&id).is_none(), "and out of the runs");
    }
}
