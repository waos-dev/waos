//! The package manager: the record of what is installed, and the one place it changes.
//!
//! The invariants a later editor will break:
//! - Everything that changes an app, or compiles its installed code, holds the app's lock
//!   ([`AppLock`], as Chrome's web-app commands take one on an app), and the app is stopped
//!   ([`StopApp`]) under it before it changes. So an open during an update opens the new version,
//!   two opens share one compile, and a removal never lands between an open's load and its start.
//!   A session's receiving, checking and compiling take no lock, since nothing installed changes
//!   then: an open of the old version is not held back for a minute's download.
//! - A session checks its update again under the lock, against what was installed meanwhile, and
//!   is cancelled until it takes the lock: once it has, it installs. A removal cancels its app's
//!   sessions and waits for them to end before it takes the lock, so no session installs the app
//!   it removed, and no file of theirs is open in the folder it moves.
//! - An install is done only once its code is compiled, so an installed app opens at once.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::future::Future;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures::StreamExt as _;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use tempfile::NamedTempFile;
use tokio::sync::{OwnedMutexGuard, mpsc, oneshot, watch};
use waos_package::{Manifest, Package, PackageError, ProgramId, PublisherKey};

use crate::packages::error::{FolderError, InstallError, RemoveError};
use crate::packages::installed::{DevelopmentBuild, InstalledProgram, InstalledVersion, Origin};
use crate::packages::programs::{self, Staged};
use crate::packages::sessions::{
    Begun, Caller, InstallProgress, InstallSession, InstallSessionInfo, Sessions,
};

/// The folder of installed apps, in the package manager's folder.
const PROGRAMS: &str = "programs";

/// Where an app being removed is moved to, in the package manager's folder, before it is removed.
const REMOVING: &str = "removing";

/// What a package being received is named with, in its app's folder.
const RECEIVING: &str = ".receiving-";

/// How much more of a package arrives before its session's watchers hear how far it is.
const RECEIVING_STEP: u64 = 256 * 1024;

/// A package's bytes, as they arrive: an error ends them, and the install with it.
pub type ByteStream = BoxStream<'static, std::io::Result<Vec<u8>>>;

/// What an install installs.
pub enum PackageContent {
    /// A `.wapk`'s bytes, and their length where known at the start (else told to the session
    /// later, [`InstallSession::set_size`]), of the app `app`: a package of another app is
    /// refused, as Android's session refuses one that is not of the package its caller named.
    /// The bytes may begin later, as a download's do; an error before any ends the install.
    Package {
        app: ProgramId,
        bytes: ByteStream,
        size: Option<u64>,
    },
    /// A build from its source, as `waos install` sends it, installed as its app's development
    /// install.
    Development(DevelopmentBuild),
}

impl PackageContent {
    /// The app it installs.
    fn app(&self) -> ProgramId {
        match self {
            PackageContent::Package { app, .. } => app.clone(),
            PackageContent::Development(build) => build.manifest.id().development(),
        }
    }

    /// The package's size where it is known at the start.
    fn size(&self) -> Option<u64> {
        match self {
            PackageContent::Package { size, .. } => *size,
            PackageContent::Development(_) => None,
        }
    }

    /// How far its install is at its start.
    fn first_progress(&self) -> InstallProgress {
        match self {
            PackageContent::Package { size, .. } => InstallProgress::Receiving {
                received: 0,
                total: *size,
            },
            PackageContent::Development(_) => InstallProgress::Installing,
        }
    }
}

/// What the package manager needs from the engine: compiling a component into its code folder.
pub trait CodeCompiler: Send + Sync {
    /// Compiles the component at `component` into `code`, its code folder. Blocks for seconds.
    fn compile(&self, component: &Path, code: &Path) -> anyhow::Result<()>;

    /// Whether `code` holds code this compiler wrote.
    fn is_usable(&self, code: &Path) -> bool;
}

/// Why an app is stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageChange {
    /// Another version of it, or another build, is installed in its place.
    Install,
    /// It is removed.
    Removal,
}

/// What the package manager needs from whoever runs apps: stopping one before it changes.
pub trait StopApp: Send + Sync {
    /// Stops the app `id`, if it runs, for `change`, and resolves once it has ended.
    fn stop<'a>(&'a self, id: &'a ProgramId, change: PackageChange) -> BoxFuture<'a, ()>;
}

/// The apps installed on this device, and every change to them.
pub struct PackageManager {
    shared: Arc<Shared>,
}

/// What the package manager and its sessions' work share.
struct Shared {
    programs: PathBuf,
    removing_folder: PathBuf,
    compiler: Arc<dyn CodeCompiler>,
    stop_app: Arc<dyn StopApp>,
    /// Each app's lock.
    locks: Mutex<HashMap<ProgramId, Arc<tokio::sync::Mutex<()>>>>,
    sessions: Sessions,
    /// Counts the changes of what is installed.
    installs: watch::Sender<u64>,
    /// Who hears each removal.
    removals: Mutex<Vec<mpsc::UnboundedSender<ProgramId>>>,
    /// The apps being removed now.
    removing: watch::Sender<BTreeSet<ProgramId>>,
}

impl PackageManager {
    /// The apps installed in `folder`, made when it is not there yet, compiled with `compiler`,
    /// stopped with `stop_app` before they change. Clears what installs, sessions and removals a
    /// crash interrupted left.
    pub fn open(
        folder: &Path,
        compiler: Arc<dyn CodeCompiler>,
        stop_app: Arc<dyn StopApp>,
    ) -> Result<PackageManager, FolderError> {
        let shared = Shared {
            programs: folder.join(PROGRAMS),
            removing_folder: folder.join(REMOVING),
            compiler,
            stop_app,
            locks: Mutex::default(),
            sessions: Sessions::default(),
            installs: watch::Sender::new(0),
            removals: Mutex::default(),
            removing: watch::Sender::default(),
        };
        fs::create_dir_all(&shared.programs).map_err(FolderError::io_at(&shared.programs))?;
        shared.clear_unfinished();
        Ok(PackageManager {
            shared: Arc::new(shared),
        })
    }

    /// Every installed app, by id. One whose files cannot be read is left out, and said so to
    /// whoever subscribes to `tracing`'s events.
    pub fn installed(&self) -> Vec<InstalledProgram> {
        let mut installed: Vec<InstalledProgram> = self
            .shared
            .program_folders()
            .iter()
            .filter_map(|folder| match programs::read_installed(folder) {
                Ok(program) => program,
                Err(error) => {
                    tracing::warn!("an installed app is left out: {error}");
                    None
                }
            })
            .collect();
        installed.sort_by(|left, right| left.manifest.id().cmp(right.manifest.id()));
        installed
    }

    /// The installed app `id`; none when it is not installed, or its files cannot be read, which
    /// is said to whoever subscribes.
    pub fn installed_program(&self, id: &ProgramId) -> Option<InstalledProgram> {
        programs::read_installed(&self.shared.program_folder(id)).unwrap_or_else(|error| {
            tracing::warn!("{id} cannot be read: {error}");
            None
        })
    }

    /// The icon of the installed app `id`, a square PNG; none when it is not installed, its
    /// development build had none, or its files cannot be read.
    pub fn icon(&self, id: &ProgramId) -> Option<PathBuf> {
        self.shared.installed_version(id)?.icon()
    }

    /// Changes each time what is installed changes: an install, an update or a removal.
    pub fn installed_changes(&self) -> watch::Receiver<u64> {
        self.shared.installs.subscribe()
    }

    /// Hears the id of each app removed from now on, once it is out of the record.
    pub fn removals(&self) -> mpsc::UnboundedReceiver<ProgramId> {
        let (sender, removals) = mpsc::unbounded_channel();
        self.shared.listeners().push(sender);
        removals
    }

    /// Whether the development build of `manifest`, under its own id, is installed already as it
    /// is: that manifest, its component of SHA-256 `component_sha256`, and `icon`, so installing
    /// it again would change nothing. Its component is not read.
    pub fn holds_development(
        &self,
        manifest: &Manifest,
        component_sha256: &[u8; 32],
        icon: Option<&[u8]>,
    ) -> bool {
        let folder = self.shared.program_folder(&manifest.id().development());
        let Ok(Some(installed)) = programs::read_installed(&folder) else {
            return false;
        };
        let same_component = installed.origin
            == Origin::Development {
                component_sha256: *component_sha256,
            };
        let installed_icon = programs::installed_folder(&folder)
            .ok()
            .flatten()
            .and_then(|unpacked| programs::read_icon(&unpacked).ok());
        installed.manifest == DevelopmentBuild::installed_manifest(manifest)
            && same_component
            && installed_icon == Some(icon.map(<[u8]>::to_vec))
    }

    /// Begins installing `content` for `caller`, a new app or an update, in a session that is
    /// among [`sessions`](Self::sessions) at once; a newer session of the same caller for the
    /// same app cancels its older one. Its work runs on a task of the tokio runtime this is
    /// called on: the package taken into the package manager's own folder, checked, unpacked and
    /// compiled; then, under the app's lock, the app stopped and the version installed. A
    /// component that does not compile is refused, and the app stays as it was.
    pub fn install(&self, content: PackageContent, caller: Caller) -> InstallSession {
        let app = content.app();
        let first = content.first_progress();
        let Begun {
            session_id,
            cancel,
            cancelled,
        } = self.shared.sessions.begin(&app, caller, first);
        let (progress, watched) = watch::channel(first);
        let (answer, answered) = oneshot::channel();
        let (size, sized) = watch::channel(content.size());
        let work = SessionWork {
            shared: Arc::clone(&self.shared),
            session_id,
            app: app.clone(),
            progress,
            size: sized,
            cancelled,
        };
        tokio::spawn(async move {
            let installed = work.install(content).await;
            match &installed {
                Ok(program) => {
                    tracing::info!(app = %work.app, version = %program.manifest.version(), "installed");
                }
                Err(error) => tracing::info!(app = %work.app, "not installed: {error}"),
            }
            drop(work);
            let _ = answer.send(installed);
        });
        InstallSession {
            app,
            caller,
            progress: watched,
            size: Arc::new(size),
            cancel,
            answer: Mutex::new(Some(answered)),
        }
    }

    /// The installs in flight now, and each change of them from now on.
    pub fn sessions(&self) -> watch::Receiver<Vec<InstallSessionInfo>> {
        self.shared.sessions.subscribe()
    }

    /// The apps being removed now, each from the call that asked until its removal ends, however
    /// it ends, and each change of them from now on.
    pub fn removing(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.shared.removing.subscribe()
    }

    /// Removes the app `id`, with everything kept in its folder: cancels its sessions at once and
    /// waits for them to end, then, under its lock, stops it and takes it out of the record in
    /// one step, and tells [`removals`](Self::removals).
    pub fn remove(
        &self,
        id: &ProgramId,
    ) -> impl Future<Output = Result<(), RemoveError>> + Send + 'static {
        let cancelled = self.shared.sessions.cancel_all_of(id);
        let removal = Removal::begin(&self.shared, id);
        let (shared, id) = (Arc::clone(&self.shared), id.clone());
        async move {
            let _removal = removal;
            shared.sessions.ended(&cancelled).await;
            let _lock = shared.lock(&id).await;
            shared.stop_app.stop(&id, PackageChange::Removal).await;
            let (folder, removing, taken) = (
                shared.program_folder(&id),
                shared.removing_folder.clone(),
                id.clone(),
            );
            blocking(move || programs::take_out(&folder, &taken, &removing)).await?;
            shared.installed_changed();
            shared
                .listeners()
                .retain(|listener| listener.send(id.clone()).is_ok());
            tracing::info!(%id, "removed");
            Ok(())
        }
    }

    /// Waits for the app `id`'s lock: until it is dropped, the app does not change, and its
    /// installed version and code are read from it.
    pub async fn lock(&self, id: &ProgramId) -> AppLock {
        Arc::clone(&self.shared).lock_owned(id).await
    }
}

/// The lock on one app, which its installs, updates, removals and starts take: the app stays as it
/// is until this is dropped. A start drops it once the app runs.
pub struct AppLock {
    _guard: OwnedMutexGuard<()>,
    shared: Arc<Shared>,
    id: ProgramId,
}

impl AppLock {
    /// The app's version installed now; none when it is not installed, or its files cannot be
    /// read.
    pub fn version(&self) -> Option<InstalledVersion> {
        self.shared.installed_version(&self.id)
    }

    /// The app's installed version, opened by `open`, which maps its code: where the code is not
    /// this compiler's, or does not open, it is compiled again first, `compiling` called as that
    /// begins, as Android compiles an app again after an update of the system; code compiled
    /// again that still does not open fails. Runs off the async task, since it may compile.
    pub async fn open<T: Send + 'static>(
        &self,
        compiling: impl FnOnce() + Send + 'static,
        open: impl Fn(&InstalledVersion) -> anyhow::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let id = &self.id;
        let version = self
            .version()
            .ok_or_else(|| anyhow::anyhow!("{id} is not installed, or its files cannot be read"))?;
        let compiler = Arc::clone(&self.shared.compiler);
        blocking(move || {
            let (component, code) = (version.component(), version.code());
            if compiler.is_usable(&code) {
                match open(&version) {
                    Ok(opened) => return Ok(opened),
                    Err(error) => tracing::warn!(
                        code = %code.display(),
                        "the code compiled does not open, so it is compiled again: {error:#}"
                    ),
                }
            }
            compiling();
            compiler.compile(&component, &code)?;
            open(&version)
        })
        .await
    }
}

/// A removal under way, among the apps being removed until it is dropped.
struct Removal {
    shared: Arc<Shared>,
    id: ProgramId,
}

impl Removal {
    fn begin(shared: &Arc<Shared>, id: &ProgramId) -> Removal {
        shared.removing.send_modify(|removing| {
            removing.insert(id.clone());
        });
        Removal {
            shared: Arc::clone(shared),
            id: id.clone(),
        }
    }
}

impl Drop for Removal {
    fn drop(&mut self) {
        self.shared.removing.send_modify(|removing| {
            removing.remove(&self.id);
        });
    }
}

/// A session's work, which ends the session when it is dropped.
struct SessionWork {
    shared: Arc<Shared>,
    session_id: u64,
    app: ProgramId,
    progress: watch::Sender<InstallProgress>,
    /// The package's size, where its caller has told it.
    size: watch::Receiver<Option<u64>>,
    cancelled: watch::Receiver<bool>,
}

impl SessionWork {
    /// Installs `content`: staged and compiled, then committed under the app's lock.
    async fn install(&self, content: PackageContent) -> Result<InstalledProgram, InstallError> {
        let staged = match content {
            PackageContent::Package { app, bytes, .. } => {
                let received = self.receive_until_cancelled(bytes).await?;
                self.set(InstallProgress::Installing);
                let (shared, folder) = (Arc::clone(&self.shared), self.shared.program_folder(&app));
                blocking(move || shared.stage_package(&app, &folder, received.path())).await?
            }
            PackageContent::Development(build) => {
                let folder = self.shared.program_folder(&self.app);
                blocking(move || programs::stage_development(&folder, &build)).await?
            }
        };
        self.check_cancelled()?;
        let compiler = Arc::clone(&self.shared.compiler);
        let staged = blocking(move || {
            let version = staged.version();
            compiler
                .compile(&version.component(), &version.code())
                .map_err(InstallError::Compile)?;
            Ok::<_, InstallError>(staged)
        })
        .await?;
        let _lock = self.shared.lock(&self.app).await;
        self.check_cancelled()?;
        self.shared
            .stop_app
            .stop(&self.app, PackageChange::Install)
            .await;
        let shared = Arc::clone(&self.shared);
        let installed = blocking(move || shared.commit(staged)).await?;
        self.shared.installed_changed();
        Ok(installed)
    }

    /// The package's bytes, taken into a file in its app's folder, unless the session is
    /// cancelled first.
    async fn receive_until_cancelled(
        &self,
        bytes: ByteStream,
    ) -> Result<NamedTempFile, InstallError> {
        let mut cancelled = self.cancelled.clone();
        tokio::select! {
            received = self.receive(bytes) => received,
            _ = cancelled.wait_for(|cancelled| *cancelled) => Err(InstallError::Cancelled),
        }
    }

    /// The package's bytes, taken into a file in its app's folder, which goes when it is dropped,
    /// or which the package manager's next opening clears; its size heard whenever its caller
    /// tells it.
    async fn receive(&self, mut bytes: ByteStream) -> Result<NamedTempFile, InstallError> {
        let folder = self.shared.program_folder(&self.app);
        fs::create_dir_all(&folder).map_err(FolderError::io_at(&folder))?;
        let mut file = tempfile::Builder::new()
            .prefix(RECEIVING)
            .tempfile_in(&folder)
            .map_err(FolderError::io_at(&folder))?;
        let mut size = self.size.clone();
        let mut received = 0;
        let mut told = 0;
        loop {
            let chunk = tokio::select! {
                chunk = bytes.next() => chunk,
                // A size told while no bytes come is told at once.
                Ok(()) = size.changed() => {
                    self.receiving(received, &size);
                    continue;
                }
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(InstallError::Receiving)?;
            // A part is written to a local file in far less time than it took to come.
            file.write_all(&chunk)
                .map_err(FolderError::io_at(file.path()))?;
            received += chunk.len() as u64;
            // A change of a step is told, not every part that arrives.
            if received / RECEIVING_STEP != told / RECEIVING_STEP {
                told = received;
                self.receiving(received, &size);
            }
        }
        self.receiving(received, &size);
        Ok(file)
    }

    /// The session has received `received` bytes, of the size `size` holds.
    fn receiving(&self, received: u64, size: &watch::Receiver<Option<u64>>) {
        self.set(InstallProgress::Receiving {
            received,
            total: *size.borrow(),
        });
    }

    /// Refuses to go on once the session is cancelled.
    fn check_cancelled(&self) -> Result<(), InstallError> {
        if *self.cancelled.borrow() {
            return Err(InstallError::Cancelled);
        }
        Ok(())
    }

    /// The session is at `progress` now; its watchers hear only a change.
    fn set(&self, progress: InstallProgress) {
        if self.progress.send_if_modified(|now| {
            let changed = *now != progress;
            *now = progress;
            changed
        }) {
            self.shared.sessions.set(self.session_id, progress);
        }
    }
}

impl Drop for SessionWork {
    fn drop(&mut self) {
        self.shared.sessions.end(self.session_id);
    }
}

impl Shared {
    /// The package received at `received`, of `app`, whose folder is `folder`, opened, checked and
    /// unpacked: a new app, or the same app and key at a newer version. Nothing installed changes.
    fn stage_package(
        &self,
        app: &ProgramId,
        folder: &Path,
        received: &Path,
    ) -> Result<Staged, InstallError> {
        let mut package = Package::open(received)?;
        let manifest = package.manifest().clone();
        if manifest.id() != app {
            return Err(InstallError::OtherApp {
                expected: app.clone(),
                package: manifest.id().clone(),
            });
        }
        let publisher = *package.publisher();
        if let Some(installed) = programs::read_installed(folder)? {
            check_update(&installed, &manifest, publisher)?;
        }
        let program = InstalledProgram {
            manifest,
            origin: Origin::Package { publisher },
        };
        programs::stage(folder, program, |unpacked| Ok(package.unpack(unpacked)?))
    }

    /// The one step that installs `staged`: its update's checks again, against what was
    /// installed meanwhile, then it is the app's installed version, and the version it replaces
    /// goes. The caller holds the app's lock.
    fn commit(&self, staged: Staged) -> Result<InstalledProgram, InstallError> {
        let folder = self.program_folder(staged.program().manifest.id());
        if let Origin::Package { publisher } = staged.program().origin
            && let Some(installed) = programs::read_installed(&folder)?
        {
            check_update(&installed, &staged.program().manifest, publisher)?;
        }
        programs::commit(&folder, staged)
    }

    /// The installed version of `id`; none when it is not installed, or its files cannot be
    /// read, which is said to whoever subscribes.
    fn installed_version(&self, id: &ProgramId) -> Option<InstalledVersion> {
        match programs::installed_folder(&self.program_folder(id)) {
            Ok(folder) => folder.map(|folder| InstalledVersion { folder }),
            Err(error) => {
                tracing::warn!("{id} cannot be read: {error}");
                None
            }
        }
    }

    /// Waits for the app `id`'s lock, and holds it until the answer is dropped.
    async fn lock(&self, id: &ProgramId) -> OwnedMutexGuard<()> {
        let lock = {
            // Every change leaves the map whole, so one that panicked leaves nothing half-changed.
            let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(locks.entry(id.clone()).or_default())
        };
        lock.lock_owned().await
    }

    /// [`lock`](Self::lock), as the app's [`AppLock`].
    async fn lock_owned(self: Arc<Self>, id: &ProgramId) -> AppLock {
        AppLock {
            _guard: self.lock(id).await,
            shared: Arc::clone(&self),
            id: id.clone(),
        }
    }

    /// Tells whoever follows what is installed that it changed.
    fn installed_changed(&self) {
        self.installs
            .send_modify(|changes| *changes = changes.wrapping_add(1));
    }

    /// Who hears each removal, for a moment: never held across an await.
    fn listeners(&self) -> MutexGuard<'_, Vec<mpsc::UnboundedSender<ProgramId>>> {
        // Every change leaves the list whole, so one that panicked leaves nothing half-changed.
        self.removals.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Removes what installs, sessions and removals a crash interrupted left: in each app's
    /// folder, and the apps being removed. What cannot be removed stays, unseen, and is said to
    /// whoever subscribes.
    fn clear_unfinished(&self) {
        for folder in self.program_folders() {
            if let Err(error) = programs::clear_unfinished(&folder) {
                tracing::warn!("what an interrupted install left stays: {error}");
            }
        }
        if let Err(error) = programs::remove_if_there(&self.removing_folder) {
            tracing::warn!("what an interrupted removal left stays: {error}");
        }
    }

    fn program_folder(&self, id: &ProgramId) -> PathBuf {
        self.programs.join(id.as_str())
    }

    /// The folder of each app installed, or left by an interrupted install, and no other file the
    /// system or a person put there; none when the folder of apps cannot be read, which is said to
    /// whoever subscribes.
    fn program_folders(&self) -> Vec<PathBuf> {
        match fs::read_dir(&self.programs) {
            Ok(entries) => entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path())
                .collect(),
            Err(error) => {
                tracing::warn!("{}: {error}", self.programs.display());
                Vec::new()
            }
        }
    }
}

/// Refuses a package of `manifest`, signed with `key`, as an update of `installed` unless it is
/// signed with the same key and is newer; a development install, which no package takes the id
/// of, is no package's to update.
fn check_update(
    installed: &InstalledProgram,
    manifest: &Manifest,
    key: PublisherKey,
) -> Result<(), InstallError> {
    let id = manifest.id();
    let Some(publisher) = installed.publisher() else {
        return Err(PackageError::DevelopmentId(id.clone()).into());
    };
    if key != publisher {
        return Err(InstallError::DifferentPublisher {
            id: id.clone(),
            installed: publisher,
            package: key,
        });
    }
    let version = manifest.version();
    if version.cmp_precedence(installed.manifest.version()) != std::cmp::Ordering::Greater {
        return Err(InstallError::NotNewer {
            id: id.clone(),
            installed: installed.manifest.version().clone(),
            package: version.clone(),
        });
    }
    Ok(())
}

/// What `work` answers, run on tokio's threads for blocking work; a panic there goes on here.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
}
