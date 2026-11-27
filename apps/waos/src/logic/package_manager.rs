//! The one place an installed app changes: its installs, updates and removals, and its code,
//! compiled into the version it belongs to at its install or, after a compiler change, at its
//! next open, as Android's package manager runs dex2oat as a step of an install and again after
//! an update of the system. What WaOS can do is the engine's (compile, load) and the store's
//! (stage, commit, remove); this decides when, and tells whoever shows it what it is doing to each
//! app ([`PackageOperation`]). An install is done only once its code is compiled, so an app on
//! the home opens at once, and a component that does not compile is refused at its install. A
//! compile at an open is the start's to show, as Optimizing (`app_starter`): a load tells its
//! caller as the compile begins.
//!
//! The invariant a later editor will break: everything that changes an app, or compiles its code,
//! holds the app's lock ([`AppLock`], as Chrome's web-app commands take one on an app), one at a
//! time, and the lock is always taken before the running apps' own turn of the app. So an open
//! during an update opens the new version, two opens share one compile, and a removal never lands
//! between an open's load and its start. A download, and the compile of a staged version, take no
//! lock, since nothing installed changes then: an open of the old version is not held back for a
//! minute's download.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context as _, Result};
use tokio::sync::{OwnedMutexGuard, watch};
use waos::{Program, WaOS};
use waos_package::{ListedProgram, Manifest, Package, ProgramId};
use waos_store::{DevelopmentBuild, InstalledProgram, InstalledVersion, Source, Staged, Store};

use super::running_apps::RunningApps;

/// How much more of a download arrives before its watchers hear how far it is.
const DOWNLOAD_STEP: u64 = 256 * 1024;

/// What the package manager is doing to an app now, in the words Android shows its user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageOperation {
    /// Its package is being downloaded: the bytes received so far, of the total its server
    /// announced, where it announced one.
    Downloading { received: u64, total: Option<u64> },
    /// Its package is unpacked and checked, and its code is being compiled; it is installed once
    /// that is done.
    Installing,
    /// It is being removed.
    Removing,
}

/// Where an install comes from.
pub(crate) enum Installable {
    /// A source's listing of it.
    Listed {
        source: Source,
        program: ListedProgram,
    },
    /// A package opened with WaOS.
    Package(Package),
    /// A build of it from its source, as `waos install` sends it, installed as its development
    /// install.
    Development {
        manifest: Manifest,
        component: Vec<u8>,
        icon: Option<Vec<u8>>,
    },
}

impl Installable {
    /// The id it is installed under.
    fn id(&self) -> ProgramId {
        match self {
            Installable::Listed { program, .. } => program.id.clone(),
            Installable::Package(package) => package.manifest().id().clone(),
            Installable::Development { manifest, .. } => manifest.id().development(),
        }
    }
}

/// The lock on one app that its install, update, removal and start take, which keeps the app as it
/// is until it is dropped: a start drops it once the app runs.
pub(crate) struct AppLock {
    _guard: OwnedMutexGuard<()>,
}

/// Installs, updates and removes apps, and compiles and loads their code, each under its app's
/// lock.
pub(crate) struct PackageManager {
    store: Store,
    engine: Arc<WaOS>,
    runs: Arc<RunningApps>,
    /// Each app's lock.
    locks: Mutex<HashMap<ProgramId, Arc<tokio::sync::Mutex<()>>>>,
    /// What is being done to which app now.
    operations: Arc<watch::Sender<BTreeMap<ProgramId, PackageOperation>>>,
}

impl PackageManager {
    /// The apps of `store`, compiled and loaded by `engine`, whose runs among `runs` stop before
    /// an app changes.
    pub(crate) fn new(store: Store, engine: Arc<WaOS>, runs: Arc<RunningApps>) -> PackageManager {
        PackageManager {
            store,
            engine,
            runs,
            locks: Mutex::default(),
            operations: Arc::new(watch::Sender::default()),
        }
    }

    /// Downloads, stages and compiles the app `from` gives, then, under its lock, stops its runs
    /// (they end `Replaced`) and commits it: a new app or an update. One whose component does not
    /// compile is refused, and the app installed stays as it was. The operation is told from the
    /// moment this is called, before the answer is first polled.
    pub(crate) fn install(
        self: &Arc<Self>,
        from: Installable,
    ) -> impl Future<Output = Result<InstalledProgram>> + Send + 'static {
        let first = match from {
            Installable::Listed { .. } => PackageOperation::Downloading {
                received: 0,
                total: None,
            },
            Installable::Package(_) | Installable::Development { .. } => {
                PackageOperation::Installing
            }
        };
        let operation = Operation::start(&self.operations, &from.id(), first);
        let packages = Arc::clone(self);
        async move { packages.install_told(from, operation).await }
    }

    /// [`install`](Self::install), told as `operation`.
    async fn install_told(
        &self,
        from: Installable,
        operation: Operation,
    ) -> Result<InstalledProgram> {
        let id = from.id();
        let staged = self.stage(from, &operation).await?;
        operation.set(PackageOperation::Installing);
        let staged = self.compile(staged).await?;
        let _lock = self.lock(&id).await;
        self.runs.stop_replaced(&id).await;
        let store = self.store.clone();
        let installed = tokio::task::spawn_blocking(move || store.commit(staged))
            .await
            .context("the install's thread")??;
        tracing::info!(%id, version = %installed.manifest.version(), "installed");
        Ok(installed)
    }

    /// Under the app `id`'s lock, stops its runs and removes it, with its code, settings and own
    /// folders. The operation is told from the moment this is called.
    pub(crate) fn remove(
        self: &Arc<Self>,
        id: &ProgramId,
    ) -> impl Future<Output = Result<()>> + Send + 'static {
        let operation = Operation::start(&self.operations, id, PackageOperation::Removing);
        let (packages, id) = (Arc::clone(self), id.clone());
        async move {
            let removed = packages.remove_now(&id).await;
            drop(operation);
            removed
        }
    }

    /// [`remove`](Self::remove), under the app's lock.
    async fn remove_now(&self, id: &ProgramId) -> Result<()> {
        let _lock = self.lock(id).await;
        self.runs.stop(id).await;
        let (store, removed) = (self.store.clone(), id.clone());
        tokio::task::spawn_blocking(move || store.remove(&removed))
            .await
            .context("the removal's thread")??;
        tracing::info!(%id, "removed");
        Ok(())
    }

    /// The app `id`'s version installed now, loaded under its lock, and the lock, held until it
    /// is dropped, and through a compile even when the caller stops waiting. Its code is a cache:
    /// where this engine's is missing or does not load it is compiled again first, `compiling`
    /// called as the compile begins, and code compiled again that still does not load fails the
    /// load.
    pub(crate) async fn load(
        &self,
        id: &ProgramId,
        compiling: impl FnOnce() + Send + 'static,
    ) -> Result<(Program, AppLock)> {
        let lock = self.lock(id).await;
        let version = self
            .store
            .installed_version(id)
            .with_context(|| format!("{id} is not installed, or its files cannot be read"))?;
        let (engine, id) = (Arc::clone(&self.engine), id.clone());
        let loading = tokio::task::spawn_blocking(move || {
            let program = load_or_compile(&engine, &version, compiling)?;
            Ok((program.named(id.as_str()), lock))
        });
        loading.await.context("the load's thread")?
    }

    /// What is being done to which app now, and each change of it from now on.
    pub(crate) fn operations(&self) -> watch::Receiver<BTreeMap<ProgramId, PackageOperation>> {
        self.operations.subscribe()
    }

    /// The version `from` gives, unpacked and checked, downloaded first from a source, telling
    /// `operation` how far the download is.
    async fn stage(&self, from: Installable, operation: &Operation) -> Result<Staged> {
        let store = self.store.clone();
        let staged = match from {
            Installable::Listed { source, program } => {
                let mut told = None;
                let download = store
                    .download(&source, &program, |progress| {
                        // A change of a step is told, not every part that arrives.
                        let step = progress.received / DOWNLOAD_STEP;
                        if told != Some(step) || Some(progress.received) == progress.total {
                            told = Some(step);
                            operation.set(PackageOperation::Downloading {
                                received: progress.received,
                                total: progress.total,
                            });
                        }
                    })
                    .await?;
                store.stage_download(download).await?
            }
            Installable::Package(package) => {
                tokio::task::spawn_blocking(move || store.stage(package))
                    .await
                    .context("the install's thread")??
            }
            Installable::Development {
                manifest,
                component,
                icon,
            } => tokio::task::spawn_blocking(move || {
                store.stage_development(&DevelopmentBuild {
                    manifest: &manifest,
                    component: &component,
                    icon: icon.as_deref(),
                })
            })
            .await
            .context("the install's thread")??,
        };
        Ok(staged)
    }

    /// `staged`, its component compiled into its code folder, off this thread.
    async fn compile(&self, staged: Staged) -> Result<Staged> {
        let engine = Arc::clone(&self.engine);
        tokio::task::spawn_blocking(move || {
            let version = staged.version();
            engine.compiler().compile(&version.component(), &version.code())?;
            Ok(staged)
        })
        .await
        .context("the compile's thread")?
    }

    /// Waits for the app `id`'s lock, and holds it until the answer is dropped.
    async fn lock(&self, id: &ProgramId) -> AppLock {
        let lock = {
            // Every change leaves the map whole, so one that panicked leaves nothing half-changed.
            let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(locks.entry(id.clone()).or_default())
        };
        AppLock {
            _guard: lock.lock_owned().await,
        }
    }
}

/// `version`'s program, mapped from this engine's code; compiled again first where that code is
/// missing or does not load, `compiling` called as the compile begins. Blocks while it compiles.
fn load_or_compile(
    engine: &WaOS,
    version: &InstalledVersion,
    compiling: impl FnOnce(),
) -> Result<Program> {
    let (component, code) = (version.component(), version.code());
    if engine.compiler().is_compiled(&code) {
        match engine.load(&component, &code) {
            Ok(program) => return Ok(program),
            Err(error) => tracing::warn!(
                code = %code.display(),
                "the code compiled does not load, so it is compiled again: {error:#}"
            ),
        }
    }
    compiling();
    engine.compiler().compile(&component, &code)?;
    engine.load(&component, &code)
}

/// An operation on one app, told to the operations' watchers from its start until it is dropped.
struct Operation {
    operations: Arc<watch::Sender<BTreeMap<ProgramId, PackageOperation>>>,
    id: ProgramId,
}

impl Operation {
    fn start(
        operations: &Arc<watch::Sender<BTreeMap<ProgramId, PackageOperation>>>,
        id: &ProgramId,
        first: PackageOperation,
    ) -> Operation {
        let operation = Operation {
            operations: Arc::clone(operations),
            id: id.clone(),
        };
        operation.set(first);
        operation
    }

    /// The operation is `now` from now on; its watchers hear only a change.
    fn set(&self, now: PackageOperation) {
        self.operations
            .send_if_modified(|operations| operations.insert(self.id.clone(), now) != Some(now));
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.operations.send_modify(|operations| {
            operations.remove(&self.id);
        });
    }
}
