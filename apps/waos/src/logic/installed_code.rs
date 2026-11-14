//! The code of the programs installed, in the engine's code cache: compiled when a program is
//! installed or updated, as Android compiles an app when it installs it, rather than at its first
//! open; compiled at an open when the cache holds nothing current, as after an update of WaOS
//! changed the engine; and removed with the program. Each compile runs on a thread of its own, and
//! which programs' code is being compiled is told (`compiling_ids`), for the library to show.
//!
//! The invariants a later editor will break: one compile of a component runs at a time, since a
//! compile asked for while one of the same component runs joins it, under the lock that lists
//! them, so an open during an install's compile waits for that compile rather than starting a
//! second; and a program is listed as compiling only while a compile of its code runs, never while
//! its code is current, so an open of a program compiled already shows nothing.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use anyhow::{Context, Result, anyhow};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::Shared;
use tokio::sync::watch;
use waos::WaOS;
use waos_package::ProgramId;

/// The compiles running, by the component each compiles.
pub(crate) struct InstalledCode {
    engine: Arc<WaOS>,
    compiles: Mutex<HashMap<PathBuf, Compile>>,
    /// The programs whose code is being compiled, for whoever shows it.
    ids: watch::Sender<BTreeSet<ProgramId>>,
    /// Whether a test holds the compiles back, so that it sees a program compiling for as long as
    /// it likes: a compile is quicker than what a test does to see it.
    #[cfg(test)]
    held: Arc<(Mutex<bool>, std::sync::Condvar)>,
}

/// One compile running: the program whose component it compiles, and its answer, which every
/// caller waiting for it shares.
struct Compile {
    id: ProgramId,
    done: CompileDone,
}

/// A compile's answer: nothing once the component's code is in the cache, or why not; cancelled
/// when its thread ended without one.
type CompileDone = Shared<oneshot::Receiver<Result<(), Arc<anyhow::Error>>>>;

impl InstalledCode {
    /// The code of the programs installed, compiled by `engine` into its code cache.
    pub(crate) fn new(engine: Arc<WaOS>) -> InstalledCode {
        InstalledCode {
            engine,
            compiles: Mutex::default(),
            ids: watch::Sender::default(),
            #[cfg(test)]
            held: Arc::default(),
        }
    }

    /// Holds every compile back from compiling until the answer is dropped; each is listed as
    /// compiling meanwhile.
    #[cfg(test)]
    pub(crate) fn hold(&self) -> HeldCompiles {
        *self.held.0.lock().unwrap() = true;
        HeldCompiles(Arc::clone(&self.held))
    }

    /// Waits while a test holds the compiles back.
    #[cfg(test)]
    fn wait_while_held(&self) {
        let (held, released) = &*self.held;
        let held = held.lock().unwrap();
        drop(released.wait_while(held, |held| *held).unwrap());
    }

    /// Starts compiling the code of the program `id`, whose component is at `component`, on a
    /// thread of its own, unless the cache holds it already or it is being compiled; the program
    /// is listed as compiling from now until its code is ready. A compile that fails is said to
    /// whoever subscribes.
    pub(crate) fn start(self: &Arc<Self>, id: &ProgramId, component: &Path) -> Result<()> {
        self.join_or_start(id, component).map(drop)
    }

    /// Resolves once the code of the program `id`, whose component is at `component`, is in the
    /// cache: at once when it is, once the compile running ends when one is, and once a compile
    /// started here ends otherwise.
    pub(crate) async fn compile(self: &Arc<Self>, id: &ProgramId, component: &Path) -> Result<()> {
        let Some(done) = self.join_or_start(id, component)? else {
            return Ok(());
        };
        match done.await {
            Ok(compiled) => compiled.map_err(|error| anyhow!("{error:#}")),
            Err(oneshot::Canceled) => Err(anyhow!("the compile's thread ended without an answer")),
        }
    }

    /// Removes the code compiled from the component at `component`, a program's that is about to
    /// be removed; the component must still be there, since its code is found by its path.
    pub(crate) fn remove(&self, component: &Path) -> Result<()> {
        self.engine.remove_compiled(component)
    }

    /// Whether the code of the program `id` is being compiled now.
    pub(crate) fn is_compiling(&self, id: &ProgramId) -> bool {
        self.ids.borrow().contains(id)
    }

    /// The programs whose code is being compiled now, and each change of them from now on.
    pub(crate) fn compiling_ids(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.ids.subscribe()
    }

    /// The answer of the compile of `component` running, or of one started now; none when the
    /// cache holds its code already.
    fn join_or_start(
        self: &Arc<Self>,
        id: &ProgramId,
        component: &Path,
    ) -> Result<Option<CompileDone>> {
        let mut compiles = self.lock();
        if let Some(running) = compiles.get(component) {
            return Ok(Some(running.done.clone()));
        }
        if self.engine.is_compiled(component)? {
            return Ok(None);
        }
        let (answer, done) = oneshot::channel();
        let done = done.shared();
        let compile = Compile {
            id: id.clone(),
            done: done.clone(),
        };
        compiles.insert(component.to_owned(), compile);
        self.tell_ids(&compiles);
        if let Err(error) = self.spawn_compile(id, component, answer) {
            compiles.remove(component);
            self.tell_ids(&compiles);
            return Err(error);
        }
        Ok(Some(done))
    }

    /// Compiles `component` on a thread of its own, then takes it out of the compiles running
    /// and sends what it came to into `answer`.
    fn spawn_compile(
        self: &Arc<Self>,
        id: &ProgramId,
        component: &Path,
        answer: oneshot::Sender<Result<(), Arc<anyhow::Error>>>,
    ) -> Result<()> {
        let (code, id, component) = (Arc::clone(self), id.clone(), component.to_owned());
        thread::Builder::new()
            .name("waos-compile".into())
            .spawn(move || {
                let finished = Finished {
                    code: &code,
                    component: &component,
                };
                tracing::info!(%id, "compiling the program's code");
                #[cfg(test)]
                code.wait_while_held();
                let compiled = code.engine.compile(&component);
                match &compiled {
                    Ok(()) => tracing::info!(%id, "the program's code is ready"),
                    Err(error) => {
                        tracing::warn!(%id, "the program's code cannot be compiled: {error:#}");
                    }
                }
                drop(finished);
                // A caller gone meanwhile waits for nothing.
                let _ = answer.send(compiled.map_err(Arc::new));
            })
            .context("starting the compile's thread")?;
        Ok(())
    }

    fn tell_ids(&self, compiles: &HashMap<PathBuf, Compile>) {
        let ids = compiles
            .values()
            .map(|compile| compile.id.clone())
            .collect();
        self.ids.send_replace(ids);
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Compile>> {
        // Every change leaves the list whole, so one that panicked leaves nothing half-changed.
        self.compiles.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Takes a compile out of the compiles running when it is dropped: when the compile ended, or
/// its thread panicked.
struct Finished<'a> {
    code: &'a InstalledCode,
    component: &'a Path,
}

impl Drop for Finished<'_> {
    fn drop(&mut self) {
        let mut compiles = self.code.lock();
        compiles.remove(self.component);
        self.code.tell_ids(&compiles);
    }
}

/// The compiles held back, until this is dropped.
#[cfg(test)]
pub(crate) struct HeldCompiles(Arc<(Mutex<bool>, std::sync::Condvar)>);

#[cfg(test)]
impl Drop for HeldCompiles {
    fn drop(&mut self) {
        let (held, released) = &*self.0;
        *held.lock().unwrap() = false;
        released.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;
    use crate::logic::test_engine::{self, NoWindows};

    fn installed_code(folder: &Path) -> Arc<InstalledCode> {
        let engine = test_engine::engine(Arc::new(NoWindows), &folder.join("code"));
        Arc::new(InstalledCode::new(Arc::new(engine)))
    }

    fn triangle() -> ProgramId {
        "dev.example.triangle".parse().unwrap()
    }

    #[test]
    fn a_compile_asked_for_while_one_runs_joins_it() {
        let folder = tempfile::tempdir().unwrap();
        let code = installed_code(folder.path());
        let component = Path::new(waos_test_programs::FOREVER);
        let held = code.hold();
        let first = code.join_or_start(&triangle(), component).unwrap().unwrap();
        let second = code.join_or_start(&triangle(), component).unwrap().unwrap();
        assert!(first.ptr_eq(&second), "one compile");
        assert!(code.is_compiling(&triangle()));
        drop(held);
        block_on(code.compile(&triangle(), component)).unwrap();
        assert!(!code.is_compiling(&triangle()));
        assert!(code.engine.is_compiled(component).unwrap());
    }

    #[test]
    fn a_program_whose_code_is_current_is_never_listed_as_compiling() {
        let folder = tempfile::tempdir().unwrap();
        let code = installed_code(folder.path());
        let component = Path::new(waos_test_programs::FOREVER);
        block_on(code.compile(&triangle(), component)).unwrap();
        let ids = code.compiling_ids();
        assert!(
            code.join_or_start(&triangle(), component)
                .unwrap()
                .is_none()
        );
        assert!(
            !ids.has_changed().unwrap(),
            "nothing listed, even for a moment"
        );
    }

    #[test]
    fn a_compile_that_fails_says_why_and_lists_the_program_no_more() {
        let folder = tempfile::tempdir().unwrap();
        let code = installed_code(folder.path());
        let component = folder.path().join("program.wasm");
        std::fs::write(&component, b"\0asm").unwrap();
        let failed = block_on(code.compile(&triangle(), &component)).unwrap_err();
        assert!(format!("{failed:#}").contains("compiling"), "{failed:#}");
        assert!(!code.is_compiling(&triangle()));
    }
}
