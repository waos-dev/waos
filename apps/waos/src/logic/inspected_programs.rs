//! The programs an inspector sees on this computer: every program installed, by the package
//! manager's manifests; and every one running, by the runs, one not installed by its id alone. Each change to them as it
//! happens, an install or a removal as the package manager announces it, whoever made it, a start
//! or an end as the runs list it;
//! each one's icon, read as it is asked for; and a program started held (`held_starts`), or ended,
//! on WaOS's own runtime, so it outlives the inspector that asked.
//!
//! The invariant: a program is named by its id, as the runs name the program they run, so what an
//! inspector reads of a running one the engine finds under that name.

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use waos::HeldProgram;
use waos::inspection::InspectedPrograms;
use waos::inspection::ProgramStorage;
use waos::inspection::bindings::wasdk::inspect::devices::{Program, ProgramChange, StartError};
use waos_package::ProgramId;
use waos_services::app_storage::AppStorage;
use waos_services::packages::{InstalledProgram, PackageManager};
use waos_services::permissions::Permissions;

use super::held_starts::HeldStarts;
use super::running_apps::RunningApps;

/// What this computer is called to an inspector, as its machine is to a program.
const THIS_COMPUTER: &str = "This computer";

/// The programs on this computer, as an inspector sees them.
pub(crate) struct ProgramsOnThisComputer {
    storage: AppStorage,
    permissions: Permissions,
    packages: Arc<PackageManager>,
    runs: Arc<RunningApps>,
    /// What starts a program held, and the runtime it starts on, which outlives the inspector
    /// that asked; served once WaOS can start programs, after the engine is made.
    starts: OnceLock<(Arc<HeldStarts>, tokio::runtime::Handle)>,
}

impl ProgramsOnThisComputer {
    pub(crate) fn new(
        storage: AppStorage,
        permissions: Permissions,
        packages: Arc<PackageManager>,
        runs: Arc<RunningApps>,
    ) -> ProgramsOnThisComputer {
        ProgramsOnThisComputer {
            storage,
            permissions,
            packages,
            runs,
            starts: OnceLock::new(),
        }
    }

    /// Starts the programs an inspector starts held with `starts`, on `runtime`.
    pub(crate) fn serve_starts(&self, starts: HeldStarts, runtime: tokio::runtime::Handle) {
        // Served once; a second serving would be the same app's.
        let _ = self.starts.set((Arc::new(starts), runtime));
    }
}

impl InspectedPrograms for ProgramsOnThisComputer {
    fn device_name(&self) -> String {
        THIS_COMPUTER.to_owned()
    }

    fn programs(&self) -> BoxFuture<'static, Vec<Program>> {
        let packages = Arc::clone(&self.packages);
        let running = self.runs.running_ids().borrow().clone();
        async move {
            // The installed programs are read from their files.
            let installed = tokio::task::spawn_blocking(move || packages.installed())
                .await
                .unwrap_or_default();
            listed(&installed, &running)
        }
        .boxed()
    }

    fn program_changes(&self) -> BoxStream<'static, ProgramChange> {
        stream::select(installs_changes(&self.packages), runs_changes(&self.runs)).boxed()
    }

    fn start_held(&self, id: &str) -> BoxFuture<'static, Result<HeldProgram, StartError>> {
        let Some((starts, runtime)) = self.starts.get() else {
            let refused = StartError::Failed("WaOS starts no program yet".to_owned());
            return futures::future::ready(Err(refused)).boxed();
        };
        let (starts, id) = (Arc::clone(starts), id.to_owned());
        let starting = runtime.spawn(async move { starts.start(&id).await });
        async move {
            starting
                .await
                .unwrap_or_else(|_| Err(StartError::Failed("the start was cut short".to_owned())))
        }
        .boxed()
    }

    fn end(&self, id: &str) -> BoxFuture<'static, ()> {
        let (Some((_, runtime)), Ok(id)) = (self.starts.get(), id.parse::<ProgramId>()) else {
            return futures::future::ready(()).boxed();
        };
        let runs = Arc::clone(&self.runs);
        let ending = runtime.spawn(async move {
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
            let path = packages.icon(&id?)?;
            // The icon is read from its file.
            tokio::task::spawn_blocking(move || std::fs::read(path).ok())
                .await
                .ok()
                .flatten()
        }
        .boxed()
    }

    fn storage(&self, id: &str) -> Option<ProgramStorage> {
        let id: ProgramId = id.parse().ok()?;
        if let Some(program) = self.runs.running(&id) {
            return Some(program.storage());
        }
        // A program that does not run is known by the folder app storage keeps of it, installed
        // or run in development before.
        if !self.storage.keeps_folders_of(&id) {
            return None;
        }
        let folders = self.storage.folders(&id).ok();
        Some(ProgramStorage::of(
            folders,
            None,
            Some(self.permissions.of(&id)),
        ))
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
fn runs_changes(runs: &RunningApps) -> BoxStream<'static, ProgramChange> {
    let ids = runs.running_ids();
    let before = ids.borrow().clone();
    stream::unfold((ids, before), |(mut ids, before)| async move {
        ids.changed().await.ok()?;
        let now = ids.borrow_and_update().clone();
        let started = now
            .difference(&before)
            .map(|id| ProgramChange::Started(id.to_string()));
        let ended = before
            .difference(&now)
            .map(|id| ProgramChange::Ended(id.to_string()));
        let changes: Vec<ProgramChange> = started.chain(ended).collect();
        Some((stream::iter(changes), (ids, now)))
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
    use crate::logic::test_installs;

    #[tokio::test]
    async fn an_inspector_sees_each_program_installed_with_its_icon_and_hears_an_install_whoever_makes_it()
     {
        let folder = tempfile::tempdir().unwrap();
        let crate::logic::app_services::Domains {
            packages,
            permissions,
            storage,
            ..
        } = test_installs::domains_never_run(&folder.path().join("store"));
        let programs = ProgramsOnThisComputer::new(
            storage,
            permissions,
            Arc::clone(&packages),
            Arc::new(RunningApps::default()),
        );
        assert!(programs.programs().await.is_empty());
        let mut changes = programs.program_changes();
        let key = SigningKey::generate();
        let package = write_package(
            folder.path(),
            "dev.example.notes",
            "Notes",
            "1.2.0",
            b"\0asm",
            &key,
        );
        // Through the package manager alone, as a headless WaOS's dev server installs, with no
        // library to pass the change on.
        test_installs::install_package(&packages, &package);
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
        assert_eq!(programs.programs().await, [notes]);
        let icon = programs.icon("dev.example.notes").await.expect("its icon");
        assert!(icon.starts_with(b"\x89PNG"), "the package's PNG");
        assert_eq!(programs.icon("dev.example.gone").await, None);
    }

    #[tokio::test]
    async fn a_program_that_does_not_run_has_the_mounts_of_its_store_folder() {
        let folder = tempfile::tempdir().unwrap();
        let crate::logic::app_services::Domains {
            packages,
            permissions,
            storage,
            ..
        } = test_installs::domains_never_run(&folder.path().join("store"));
        let key = SigningKey::generate();
        let package = write_package(
            folder.path(),
            "dev.example.notes",
            "Notes",
            "1.2.0",
            b"\0asm",
            &key,
        );
        test_installs::install_package(&packages, &package);
        let id: waos_package::ProgramId = "dev.example.notes".parse().unwrap();
        let folders = storage.folders(&id).unwrap();
        std::fs::write(folders.data.join("a note.txt"), b"hi").unwrap();

        let programs = ProgramsOnThisComputer::new(
            storage,
            permissions,
            packages,
            Arc::new(RunningApps::default()),
        );
        let storage = programs.storage("dev.example.notes").expect("its storage");
        let mounts = storage.mounts();
        let data = mounts
            .iter()
            .find(|mount| mount.path == "/data")
            .expect("its own data folder");
        assert_eq!(
            data.kind,
            waos::inspection::bindings::wasdk::inspect::storage::MountKind::OwnData
        );
        assert!(data.writable);
        assert_eq!(data.place, folders.data.display().to_string());
        let entries = storage.read_folder("/data").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "a note.txt");
        assert!(!entries[0].is_folder);
        assert_eq!(entries[0].size, Some(2));
        assert!(storage.read_folder("/data/../..").is_err());
        assert!(programs.storage("dev.example.gone").is_none());
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
