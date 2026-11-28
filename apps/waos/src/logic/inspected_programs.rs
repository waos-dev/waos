//! The programs an inspector sees on this computer: every program installed, by the package
//! manager's manifests; every one running, by the runs; and every one run in development the store keeps days
//! of, though it has ended; a program not installed by its id alone. Each change to them as it
//! happens, an install or a removal as the library makes it, a start or an end as the runs list it;
//! each one's icon, read as it is asked for; and a program started held (`held_starts`), or ended,
//! on WaOS's own runtime, so it outlives the inspector that asked.
//!
//! The invariant: a program is named by its id, as the runs name the program they run, so what an
//! inspector reads of a running one the engine finds under that name.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use futures::channel::mpsc::{self, UnboundedSender};
use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use waos::HeldProgram;
use waos::inspection::InspectedPrograms;
use waos::inspection::ProgramStorage;
use waos::inspection::bindings::wasdk::inspect::devices::{Program, ProgramChange, StartError};
use waos_package::ProgramId;
use waos_services::packages::{InstalledProgram, PackageManager};
use waos_services::permissions::Permissions;
use waos_services::store::Store;

use super::held_starts::HeldStarts;
use super::running_apps::RunningApps;

/// What this computer is called to an inspector, as its machine is to a program.
const THIS_COMPUTER: &str = "This computer";

/// The programs on this computer, as an inspector sees them.
pub(crate) struct ProgramsOnThisComputer {
    store: Store,
    permissions: Permissions,
    packages: Arc<PackageManager>,
    runs: Arc<RunningApps>,
    /// The programs installed as the library last said, which a change is told against.
    installed: Mutex<BTreeSet<ProgramId>>,
    /// Who hears each install and removal.
    listeners: Mutex<Vec<UnboundedSender<ProgramChange>>>,
    /// What starts a program held, and the runtime it starts on, which outlives the inspector
    /// that asked; served once WaOS can start programs, after the engine is made.
    starts: OnceLock<(Arc<HeldStarts>, tokio::runtime::Handle)>,
}

impl ProgramsOnThisComputer {
    pub(crate) fn new(
        store: Store,
        permissions: Permissions,
        packages: Arc<PackageManager>,
        runs: Arc<RunningApps>,
    ) -> ProgramsOnThisComputer {
        let installed = ids_of(&packages.installed());
        ProgramsOnThisComputer {
            store,
            permissions,
            packages,
            runs,
            installed: Mutex::new(installed),
            listeners: Mutex::default(),
            starts: OnceLock::new(),
        }
    }

    /// Starts the programs an inspector starts held with `starts`, on `runtime`.
    pub(crate) fn serve_starts(&self, starts: HeldStarts, runtime: tokio::runtime::Handle) {
        // Served once; a second serving would be the same app's.
        let _ = self.starts.set((Arc::new(starts), runtime));
    }

    /// The library installed or removed a program: each listener hears what is new and what is
    /// gone since it last said.
    pub(crate) fn installed_changed(&self) {
        let installed = self.packages.installed();
        let now = ids_of(&installed);
        let before = std::mem::replace(&mut *locked(&self.installed), now.clone());
        let added = installed
            .iter()
            .filter(|program| !before.contains(program.manifest.id()))
            .map(|program| ProgramChange::Added(program_of(program, false)));
        let removed = before
            .difference(&now)
            .map(|id| ProgramChange::Removed(id.to_string()));
        let changes: Vec<ProgramChange> = added.chain(removed).collect();
        locked(&self.listeners).retain(|listener| {
            changes
                .iter()
                .all(|change| listener.unbounded_send(change.clone()).is_ok())
        });
    }
}

impl InspectedPrograms for ProgramsOnThisComputer {
    fn device_name(&self) -> String {
        THIS_COMPUTER.to_owned()
    }

    fn programs(&self) -> BoxFuture<'static, Vec<Program>> {
        let (store, packages) = (self.store.clone(), Arc::clone(&self.packages));
        let running = self.runs.running_ids().borrow().clone();
        async move {
            // The installed programs and the days kept are read from their files.
            let (installed, recorded) = tokio::task::spawn_blocking(move || {
                (packages.installed(), store.programs_with_days())
            })
            .await
            .unwrap_or_default();
            listed(&installed, &running, &recorded)
        }
        .boxed()
    }

    fn program_changes(&self) -> BoxStream<'static, ProgramChange> {
        let (sender, installs) = mpsc::unbounded();
        locked(&self.listeners).push(sender);
        stream::select(installs, runs_changes(&self.runs)).boxed()
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
        // A program that does not run is known by the folder the store keeps of it, installed
        // or run in development before.
        if !self.store.keeps_folders_of(&id) {
            return None;
        }
        let folders = self.store.folders(&id).ok();
        Some(ProgramStorage::of(
            folders,
            None,
            Some(self.permissions.of(&id)),
        ))
    }
}

/// Every program installed; then each one not installed, by its id alone: those running, then
/// those ended that days are kept of.
fn listed(
    installed: &[InstalledProgram],
    running: &BTreeSet<ProgramId>,
    recorded: &[String],
) -> Vec<Program> {
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
    let ended = recorded.iter().filter(|name| {
        let id: Option<ProgramId> = name.parse().ok();
        id.is_some_and(|id| !installed_ids.contains(&id) && !running.contains(&id))
    });
    programs.extend(ended.map(|name| run_in_development(name.clone(), false)));
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

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
    async fn an_inspector_sees_each_program_installed_with_its_icon_and_hears_an_install() {
        let folder = tempfile::tempdir().unwrap();
        let crate::logic::app_services::Domains {
            packages,
            permissions,
            store,
        } = test_installs::domains_never_run(&folder.path().join("store"));
        let programs = ProgramsOnThisComputer::new(
            store,
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
        test_installs::install_package(&packages, &package);
        programs.installed_changed();
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
            store,
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
        let folders = store.folders(&id).unwrap();
        std::fs::write(folders.data.join("a note.txt"), b"hi").unwrap();

        let programs = ProgramsOnThisComputer::new(
            store,
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
            listed(&[], &running, &["dev.example.forever".to_owned()]),
            [Program {
                id: "dev.example.forever".to_owned(),
                name: "dev.example.forever".to_owned(),
                version: None,
                running: true,
            }],
            "once, though days are kept of it too"
        );
    }

    #[test]
    fn a_development_run_that_ended_stays_listed_while_days_are_kept_of_it() {
        let recorded = ["dev.example.camera".to_owned()];
        assert_eq!(
            listed(&[], &BTreeSet::new(), &recorded),
            [Program {
                id: "dev.example.camera".to_owned(),
                name: "dev.example.camera".to_owned(),
                version: None,
                running: false,
            }]
        );
    }
}
