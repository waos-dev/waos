//! The programs an inspector sees on this computer (decision 12 of
//! `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`): every program installed,
//! by the store's manifests, and every one running, by the runs, a development run of a program
//! not installed by its id alone; each change to them as it happens, an install or a removal as
//! the library makes it, a start or an end as the runs list it; and each one's icon, read as it is
//! asked for.
//!
//! The invariant: a program is named by its id, as the runs name the program they run, so what an
//! inspector reads of a running one the engine finds under that name.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures::channel::mpsc::{self, UnboundedSender};
use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use waos::inspection::InspectedPrograms;
use waos::inspection::bindings::wasdk::inspect::devices::{Program, ProgramChange};
use waos_package::ProgramId;
use waos_store::{InstalledProgram, Store};

use super::runs::Runs;

/// What this computer is called to an inspector, as its machine is to a program.
const THIS_COMPUTER: &str = "This computer";

/// The programs on this computer, as an inspector sees them.
pub(crate) struct ProgramsOnThisComputer {
    store: Store,
    runs: Arc<Runs>,
    /// The programs installed as the library last said, which a change is told against.
    installed: Mutex<BTreeSet<ProgramId>>,
    /// Who hears each install and removal.
    listeners: Mutex<Vec<UnboundedSender<ProgramChange>>>,
}

impl ProgramsOnThisComputer {
    pub(crate) fn new(store: Store, runs: Arc<Runs>) -> ProgramsOnThisComputer {
        let installed = ids_of(&store.installed());
        ProgramsOnThisComputer {
            store,
            runs,
            installed: Mutex::new(installed),
            listeners: Mutex::default(),
        }
    }

    /// The library installed or removed a program: each listener hears what is new and what is
    /// gone since it last said.
    pub(crate) fn installed_changed(&self) {
        let installed = self.store.installed();
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
        let store = self.store.clone();
        let running = self.runs.running_ids().borrow().clone();
        async move {
            // The installed programs are read from their files.
            let installed = tokio::task::spawn_blocking(move || store.installed())
                .await
                .unwrap_or_default();
            listed(&installed, &running)
        }
        .boxed()
    }

    fn program_changes(&self) -> BoxStream<'static, ProgramChange> {
        let (sender, installs) = mpsc::unbounded();
        locked(&self.listeners).push(sender);
        stream::select(installs, runs_changes(&self.runs)).boxed()
    }

    fn icon(&self, id: &str) -> BoxFuture<'static, Option<Vec<u8>>> {
        let store = self.store.clone();
        let id: Option<ProgramId> = id.parse().ok();
        async move {
            let path = store.icon(&id?)?;
            // The icon is read from its file.
            tokio::task::spawn_blocking(move || std::fs::read(path).ok())
                .await
                .ok()
                .flatten()
        }
        .boxed()
    }
}

/// Every program installed, and then each running one not installed, by its id alone.
fn listed(installed: &[InstalledProgram], running: &BTreeSet<ProgramId>) -> Vec<Program> {
    let installed_ids = ids_of(installed);
    let mut programs: Vec<Program> = installed
        .iter()
        .map(|program| program_of(program, running.contains(program.manifest.id())))
        .collect();
    programs.extend(running.difference(&installed_ids).map(|id| Program {
        id: id.to_string(),
        name: id.to_string(),
        version: None,
        running: true,
    }));
    programs
}

/// Each start and end of a run from now on, as the runs list them.
fn runs_changes(runs: &Runs) -> BoxStream<'static, ProgramChange> {
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
        name: program.manifest.name().to_owned(),
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
    use crate::logic::placement::tests::store_in;

    #[tokio::test]
    async fn an_inspector_sees_each_program_installed_with_its_icon_and_hears_an_install() {
        let folder = tempfile::tempdir().unwrap();
        let store = store_in(folder.path());
        let programs = ProgramsOnThisComputer::new(store.clone(), Arc::new(Runs::default()));
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
        store
            .install(waos_package::Package::open(&package).unwrap())
            .unwrap();
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
