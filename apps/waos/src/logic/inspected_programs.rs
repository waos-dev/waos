//! The apps an inspector sees on this computer, which the app instances hand the instance of an
//! app that asks for the inspector role: every app installed, by the package manager's
//! manifests; and every one running, by the app instances, one not installed by its id alone. Each
//! change to them as it happens, an install or a removal as the package manager announces it,
//! whoever made it, a start or an end as the app instances list it; each one's icon, read as it is
//! asked for; an app started held (`held_starts`), or ended, on the runtime the inspector was
//! started on, so it outlives the inspector that asked; and what an app that does not run sees of
//! the files, by the folders app storage keeps and its permissions.
//!
//! The invariant: an app is named by its id, as the app instances name the instance they run, so
//! what an inspector reads of a running one the engine finds under that name.

use std::collections::BTreeSet;
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use waos_capabilities::wit::wasdk::inspect::devices::{Program, ProgramChange, StartError};
use waos_capabilities::{AppFiles, AppHold, InspectedApps};
use waos_package::ProgramId;
use waos_services::app_storage::AppStorage;
use waos_services::packages::{InstalledProgram, PackageManager};
use waos_services::permissions::Permissions;

use super::app_instances::{AppInstances, RunningInstances};
use super::held_starts::HeldStarts;

/// What this computer is called to an inspector, as its machine is to a program.
const THIS_COMPUTER: &str = "This computer";

/// The apps on this computer, as an inspector sees them.
pub(crate) struct ProgramsOnThisComputer {
    storage: AppStorage,
    permissions: Permissions,
    packages: Arc<PackageManager>,
    runs: Arc<AppInstances>,
    /// What starts an app held.
    starts: HeldStarts,
    /// The runtime a start or an end runs on, which outlives the inspector that asked.
    runtime: tokio::runtime::Handle,
}

impl ProgramsOnThisComputer {
    /// The apps `starts`' package manager installs and those its app instances run, each with its
    /// own folders in `storage` and what `permissions` let it use, opened held by `starts` and
    /// ended on `runtime`.
    pub(crate) fn new(
        storage: AppStorage,
        permissions: Permissions,
        starts: HeldStarts,
        runtime: tokio::runtime::Handle,
    ) -> ProgramsOnThisComputer {
        ProgramsOnThisComputer {
            storage,
            permissions,
            packages: Arc::clone(&starts.packages),
            runs: Arc::clone(&starts.instances),
            starts,
            runtime,
        }
    }
}

impl InspectedApps for ProgramsOnThisComputer {
    fn device_name(&self) -> String {
        THIS_COMPUTER.to_owned()
    }

    fn apps(&self) -> BoxFuture<'static, Vec<Program>> {
        let packages = Arc::clone(&self.packages);
        let running = ids_running(&self.runs.running().borrow());
        async move {
            // The installed apps are read from their files.
            let installed = tokio::task::spawn_blocking(move || packages.installed())
                .await
                .unwrap_or_default();
            listed(&installed, &running)
        }
        .boxed()
    }

    fn app_changes(&self) -> BoxStream<'static, ProgramChange> {
        stream::select(installs_changes(&self.packages), runs_changes(&self.runs)).boxed()
    }

    fn start_held(&self, id: &str) -> BoxFuture<'static, Result<AppHold, StartError>> {
        let (starts, id) = (self.starts.clone(), id.to_owned());
        let starting = self.runtime.spawn(async move { starts.start(&id).await });
        async move {
            starting
                .await
                .unwrap_or_else(|_| Err(StartError::Failed("the start was cut short".to_owned())))
        }
        .boxed()
    }

    fn end(&self, id: &str) -> BoxFuture<'static, ()> {
        let Ok(id) = id.parse::<ProgramId>() else {
            return futures::future::ready(()).boxed();
        };
        let runs = Arc::clone(&self.runs);
        let ending = self.runtime.spawn(async move {
            tracing::info!(%id, "ended from an inspector");
            runs.stop(&id).await;
        });
        async move {
            // A cut short end has nothing more to wait for.
            let _ = ending.await;
        }
        .boxed()
    }

    fn icon(&self, id: &str) -> BoxFuture<'static, Option<Vec<u8>>> {
        let packages = Arc::clone(&self.packages);
        let id: Option<ProgramId> = id.parse().ok();
        async move {
            let id = id?;
            // The package manager may read the icon from its file.
            tokio::task::spawn_blocking(move || packages.icon(&id))
                .await
                .ok()
                .flatten()
                .map(|icon| icon.to_vec())
        }
        .boxed()
    }

    fn files(&self, id: &str) -> Option<AppFiles> {
        let id: ProgramId = id.parse().ok()?;
        // An app that does not run is known by the folder app storage keeps of it, installed or
        // run in development before.
        if !self.storage.keeps_folders_of(&id) {
            return None;
        }
        Some(AppFiles {
            folders: self.storage.folders(&id).ok(),
            grants: Some(self.permissions.of(&id)),
        })
    }
}

/// Every program installed; then each one running not installed, by its id alone.
fn listed(installed: &[InstalledProgram], running: &BTreeSet<ProgramId>) -> Vec<Program> {
    let installed_ids = ids_of(installed);
    let mut programs: Vec<Program> = installed
        .iter()
        .map(|program| program_of(program, running.contains(program.manifest.id())))
        .collect();
    programs.extend(
        running
            .difference(&installed_ids)
            .map(|id| run_in_development(id.to_string(), true)),
    );
    programs
}

/// A program run in development, not installed, by its id alone.
fn run_in_development(id: String, running: bool) -> Program {
    Program {
        name: id.clone(),
        id,
        version: None,
        running,
    }
}

/// Each install and removal from now on, as the package manager announces them: what is new and
/// what is gone since the change before, read from the package manager's files.
fn installs_changes(packages: &Arc<PackageManager>) -> BoxStream<'static, ProgramChange> {
    let mut changes = packages.installed_changes();
    // Seen before what is installed is read, so a change between the two is heard.
    changes.borrow_and_update();
    let before = ids_of(&packages.installed());
    let packages = Arc::clone(packages);
    stream::unfold(
        (changes, before, packages),
        |(mut changes, before, packages)| async move {
            changes.changed().await.ok()?;
            changes.borrow_and_update();
            let reading = Arc::clone(&packages);
            let installed = tokio::task::spawn_blocking(move || reading.installed())
                .await
                .unwrap_or_default();
            let now = ids_of(&installed);
            let added = installed
                .iter()
                .filter(|program| !before.contains(program.manifest.id()))
                .map(|program| ProgramChange::Added(program_of(program, false)));
            let removed = before
                .difference(&now)
                .map(|id| ProgramChange::Removed(id.to_string()));
            let told: Vec<ProgramChange> = added.chain(removed).collect();
            Some((stream::iter(told), (changes, now, packages)))
        },
    )
    .flatten()
    .boxed()
}

/// Each start and end of a run from now on, as the runs list them.
fn runs_changes(runs: &AppInstances) -> BoxStream<'static, ProgramChange> {
    let running = runs.running();
    let before = ids_running(&running.borrow());
    stream::unfold((running, before), |(mut running, before)| async move {
        running.changed().await.ok()?;
        let now = ids_running(&running.borrow_and_update());
        let started = now
            .difference(&before)
            .map(|id| ProgramChange::Started(id.to_string()));
        let ended = before
            .difference(&now)
            .map(|id| ProgramChange::Ended(id.to_string()));
        let changes: Vec<ProgramChange> = started.chain(ended).collect();
        Some((stream::iter(changes), (running, now)))
    })
    .flatten()
    .boxed()
}

fn program_of(program: &InstalledProgram, running: bool) -> Program {
    Program {
        id: program.manifest.id().to_string(),
        name: program.name(),
        version: Some(program.manifest.version().to_string()),
        running,
    }
}

fn ids_running(running: &RunningInstances) -> BTreeSet<ProgramId> {
    running.keys().cloned().collect()
}

fn ids_of(installed: &[InstalledProgram]) -> BTreeSet<ProgramId> {
    installed
        .iter()
        .map(|program| program.manifest.id().clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use waos_package::SigningKey;

    use super::*;
    use crate::logic::opened_package::tests::write_package;
    use crate::logic::startup::tests::start_asking_nobody;
    use crate::logic::startup::{Setup, Started};
    use crate::logic::test_engine::{self, NoWindows};
    use crate::logic::test_installs;

    /// What an inspector sees of a test's WaOS in `folder`, with nothing running.
    fn programs_of(folder: &std::path::Path) -> (Started, ProgramsOnThisComputer) {
        let setup = Setup::in_memory(folder, tokio::runtime::Handle::current());
        let started = start_asking_nobody(setup, |_before| {
            let shell = test_engine::shell(Arc::new(NoWindows));
            (shell, test_engine::no_screens())
        });
        let starts = HeldStarts {
            instances: Arc::clone(&started.instances),
            packages: Arc::clone(&started.packages),
        };
        let programs = ProgramsOnThisComputer::new(
            started.storage.clone(),
            started.permissions.clone(),
            starts,
            tokio::runtime::Handle::current(),
        );
        (started, programs)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_inspector_sees_each_program_installed_with_its_icon_and_hears_an_install_whoever_makes_it()
     {
        let folder = tempfile::tempdir().unwrap();
        let (started, programs) = programs_of(folder.path());
        assert!(programs.apps().await.is_empty());
        let mut changes = programs.app_changes();
        let key = SigningKey::generate();
        let package = write_package(
            folder.path(),
            "dev.example.notes",
            "Notes",
            "1.2.0",
            test_installs::EMPTY_COMPONENT,
            &key,
        );
        // Through the package manager alone, as a headless WaOS's dev server installs, with no
        // library to pass the change on.
        test_installs::install_package(&started.packages, &package);
        let notes = Program {
            id: "dev.example.notes".to_owned(),
            name: "Notes".to_owned(),
            version: Some("1.2.0".to_owned()),
            running: false,
        };
        assert_eq!(
            changes.next().await,
            Some(ProgramChange::Added(notes.clone()))
        );
        assert_eq!(programs.apps().await, [notes]);
        let icon = programs.icon("dev.example.notes").await.expect("its icon");
        assert!(icon.starts_with(b"\x89PNG"), "the package's PNG");
        assert_eq!(programs.icon("dev.example.gone").await, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_app_that_does_not_run_has_its_own_folders_and_its_permissions() {
        let folder = tempfile::tempdir().unwrap();
        let (started, programs) = programs_of(folder.path());
        let key = SigningKey::generate();
        let package = write_package(
            folder.path(),
            "dev.example.notes",
            "Notes",
            "1.2.0",
            test_installs::EMPTY_COMPONENT,
            &key,
        );
        test_installs::install_package(&started.packages, &package);
        let id: ProgramId = "dev.example.notes".parse().unwrap();
        let folders = started.storage.folders(&id).unwrap();

        let files = programs.files("dev.example.notes").expect("its files");
        assert_eq!(files.folders, Some(folders), "app storage's folders of it");
        assert!(files.grants.is_some(), "and what its user lets it use");
        assert!(programs.files("dev.example.gone").is_none());
    }

    #[test]
    fn a_run_of_a_program_not_installed_is_listed_by_its_id() {
        let running = BTreeSet::from(["dev.example.forever".parse().unwrap()]);
        assert_eq!(
            listed(&[], &running),
            [Program {
                id: "dev.example.forever".to_owned(),
                name: "dev.example.forever".to_owned(),
                version: None,
                running: true,
            }]
        );
    }
}
