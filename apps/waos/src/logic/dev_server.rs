//! Where `waos` and `waos open` reach this WaOS: a listener on the socket `waos-dev-protocol`
//! names, on a thread of the app's own with a runtime of its own, and a task per connection,
//! each, once the hellos are exchanged, one request: an install or an uninstall
//! (`dev_install`), a launch or a stop (`dev_launch`), an attach to what a launch writes
//! (`dev_attach`), a call of a program's service extension or the list of
//! the programs serving them (`dev_extensions`), one opening of files with a program (`opening`), one screenshot of the windows
//! of a program running (`dev_screenshot`), one input to one of them (`dev_input`), a size for one
//! of them (`dev_resize`), a program's settings read or one of them set (`dev_settings`), or an
//! inspection of a program (`dev_inspect`). While developer access is on, it takes connections
//! from the network too (`dev_network`), each answered only once its hello carries a paired
//! computer's token, but for a pairing, which makes one (`dev_pairing`). A connection of another protocol's major, or with a hello this build cannot
//! read, ends before it asks anything. Stopping the server stops every run, waits for each
//! connection to say so, and removes the socket file.
//!
//! The invariants: the server's thread owns the listener, so the socket file goes with the
//! thread, and dropping the server returns only once the thread has: a WaOS that quit leaves no
//! socket file behind. A connection from the network is refused anything but a pairing before its
//! token is known, and ends once access is withdrawn: turned off, or its computer removed.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::app_instances::AppInstances;
use super::asking_now::AskingNow;
use super::dev_attach::{self, AttachContext};
use super::dev_extensions;
use super::dev_inspect::{self, InspectionContext};
use super::dev_install::{self, InstallContext};
use super::dev_launch::{self, LaunchContext};
use super::dev_network::NetworkDoor;
use super::dev_pairing;
use super::dev_refusal;
use super::dev_settings::{self, SettingsContext};
use super::developer_access::DeveloperAccess;
use super::environments::ProgramEnvironments;
use super::opening;
use super::{dev_input, dev_resize, dev_screenshot};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinSet;
use waos::bindings::wasdk::platform::windows::Size;
use waos::{OpenRequest, WaOS};
use waos_dev_protocol::{
    Hello, Incoming, Listener, Locality, MAJOR, RefusedAnswer, Request, ServerConnection,
};
use waos_services::packages::PackageManager;
use waos_services::permissions::Permissions;

/// This build, as its hello names it: "WaOS 0.3.0".
pub(crate) const BUILD: &str = concat!("WaOS ", env!("CARGO_PKG_VERSION"));

/// How long the connections may take to say their runs ended, once every program is stopped.
const CLOSING_WAIT: Duration = Duration::from_secs(2);

/// How long a connection from the network may take over its TLS and WebSocket handshakes.
const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);

/// The domains the dev server serves `waos` with, each handed to it by name.
#[derive(Clone)]
pub(crate) struct ServedDomains {
    pub(crate) instances: Arc<AppInstances>,
    pub(crate) packages: Arc<PackageManager>,
    pub(crate) permissions: Permissions,
    pub(crate) environments: Arc<ProgramEnvironments>,
    /// The engine, which counts today's activity of each app, for an inspection.
    pub(crate) engine: Arc<WaOS>,
}

/// The server, listening until dropped. Dropping it stops every run, waits for the connections
/// to hear so, removes the socket file, and returns once all that is done.
pub(crate) struct DevServer {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    /// How many connections are open now; changes on each accept and close so headless idle
    /// shutdown knows when the server is quiet.
    active_connections: watch::Receiver<usize>,
}

impl DevServer {
    /// Listens at `socket` for `waos`'s requests on the apps `served` keeps: installing what it
    /// sends as development installs, launching any installed app through its starter, and
    /// telling each attach what `asking` says its app's user is asked; opening files with an app,
    /// installed and opened, or running; reading and driving any program running; and reading and
    /// setting the settings of a development program. Takes connections from the network too
    /// while `access` is on. Refused, as `Listener::bind` refuses, while another WaOS answers
    /// there.
    pub(crate) fn start(
        served: ServedDomains,
        asking: Arc<AskingNow>,
        socket: &Path,
        access: DeveloperAccess,
    ) -> io::Result<DevServer> {
        let (stop, stopped) = oneshot::channel();
        let (bound, binding) = mpsc::channel();
        let (connections_tx, active_connections) = watch::channel(0usize);
        let context = ServerContext {
            served,
            asking,
            socket: socket.to_owned(),
            access,
            connections: Arc::new(connections_tx),
        };
        let thread = thread::Builder::new()
            .name("dev server".into())
            .spawn(move || serve_on_this_thread(context, &bound, stopped))?;
        match binding.recv() {
            Ok(Ok(())) => Ok(DevServer {
                stop: Some(stop),
                thread: Some(thread),
                active_connections,
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_thread_gone) => {
                let _ = thread.join();
                Err(io::Error::other(
                    "the dev server's thread ended before it bound",
                ))
            }
        }
    }

    /// How many client connections are currently open, changing on each accept and close.
    pub(crate) fn active_connections(&self) -> watch::Receiver<usize> {
        self.active_connections.clone()
    }
}

impl Drop for DevServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            // A thread that ended already has nothing to stop.
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// What the server's thread serves with.
struct ServerContext {
    /// The apps running, installed and started, and the engine they run on.
    served: ServedDomains,
    /// What each program's user is asked now.
    asking: Arc<AskingNow>,
    socket: PathBuf,
    /// Whether computers on the network may connect, and which.
    access: DeveloperAccess,
    /// Number of currently open client connections.
    connections: Arc<watch::Sender<usize>>,
}

/// The server's thread: binds, reports whether it could, and serves until told to stop.
fn serve_on_this_thread(
    context: ServerContext,
    bound: &mpsc::Sender<io::Result<()>>,
    stopped: oneshot::Receiver<()>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = bound.send(Err(error));
            return;
        }
    };
    runtime.block_on(async {
        let listener = match Listener::bind(&context.socket).await {
            Ok(listener) => listener,
            Err(error) => {
                let _ = bound.send(Err(error));
                return;
            }
        };
        let _ = bound.send(Ok(()));
        serve(listener, context, stopped).await;
    });
}

/// Accepts connections until `stopped`, then stops every run and waits for the connections to
/// close.
async fn serve(mut listener: Listener, context: ServerContext, mut stopped: oneshot::Receiver<()>) {
    let mut connections = JoinSet::new();
    let mut network = NetworkDoor::new(context.access.clone());
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(connection) => {
                    let guard = ActiveConnection::enter(Arc::clone(&context.connections));
                    let conn_ctx = connection_context(&context);
                    connections.spawn(async move {
                        let _guard = guard;
                        serve_connection(connection, conn_ctx).await;
                    });
                }
                Err(error) => {
                    tracing::warn!(%error, "accepting a connection");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            incoming = network.next() => match incoming {
                Ok(incoming) => {
                    let guard = ActiveConnection::enter(Arc::clone(&context.connections));
                    let conn_ctx = connection_context(&context);
                    connections.spawn(async move {
                        let _guard = guard;
                        serve_from_network(incoming, conn_ctx).await;
                    });
                }
                Err(error) => {
                    tracing::warn!(%error, "accepting a connection from the network");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            _ = &mut stopped => break,
        }
    }
    context.served.instances.stop_all().await;
    let closing = async { while connections.join_next().await.is_some() {} };
    if tokio::time::timeout(CLOSING_WAIT, closing).await.is_err() {
        tracing::warn!("some connections did not close in time");
    }
    drop(listener);
}

/// Counts one active connection in `connections` until dropped.
struct ActiveConnection(Arc<watch::Sender<usize>>);

impl ActiveConnection {
    fn enter(connections: Arc<watch::Sender<usize>>) -> ActiveConnection {
        connections.send_modify(|count| *count += 1);
        ActiveConnection(connections)
    }
}

impl Drop for ActiveConnection {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count = count.saturating_sub(1));
    }
}

/// What one connection is served with.
struct ConnectionContext {
    served: ServedDomains,
    asking: Arc<AskingNow>,
    access: DeveloperAccess,
}

fn connection_context(context: &ServerContext) -> ConnectionContext {
    ConnectionContext {
        served: context.served.clone(),
        asking: Arc::clone(&context.asking),
        access: context.access.clone(),
    }
}

impl ConnectionContext {
    fn install(&self) -> InstallContext {
        InstallContext {
            packages: Arc::clone(&self.served.packages),
        }
    }

    fn launch(&self) -> LaunchContext {
        LaunchContext {
            instances: Arc::clone(&self.served.instances),
            packages: Arc::clone(&self.served.packages),
        }
    }

    fn settings(&self) -> SettingsContext {
        SettingsContext {
            permissions: self.served.permissions.clone(),
            environments: Arc::clone(&self.served.environments),
        }
    }
}

/// Serves a connection from the network once its handshakes are done, as one from this machine.
async fn serve_from_network(incoming: Incoming, context: ConnectionContext) {
    let peer = incoming.peer();
    match tokio::time::timeout(HANDSHAKE_WAIT, incoming.handshake()).await {
        Ok(Ok(connection)) => serve_connection(connection, context).await,
        Ok(Err(error)) => {
            tracing::info!(%peer, %error, "a connection from the network failed its handshake")
        }
        Err(_) => {
            tracing::info!(%peer, "a connection from the network took too long over its handshake")
        }
    }
}

/// Serves `connection` to its end: the request its first message asks for, or nothing when it
/// asks for none, after the hellos say the two halves speak the same protocol and, from the
/// network, that the computer is paired.
async fn serve_connection(mut connection: ServerConnection, context: ConnectionContext) {
    let Some(theirs) = greeted(&mut connection).await else {
        return;
    };
    let request = match connection.receive::<Request>().await {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(%error, "a connection ended before it asked anything");
            return;
        }
    };
    if !admitted(&connection, &theirs, &request, &context.access) {
        let refusal = dev_refusal::not_paired();
        let _ = connection.send(&RefusedAnswer::Refused(refusal)).await;
        return;
    }
    match connection.locality() {
        Locality::ThisMachine => serve_request(connection, request, &context).await,
        Locality::Network => {
            let token = theirs.token.as_ref();
            tokio::select! {
                () = serve_request(connection, request, &context) => {}
                () = context.access.withdrawn(token) => {
                    tracing::info!("a connection from the network ended: its access was withdrawn");
                }
            }
        }
    }
}

/// Their hello, once the two halves agree they speak the same protocol; none when they do not.
async fn greeted(connection: &mut ServerConnection) -> Option<Hello> {
    let theirs = match connection.accept_hello(&Hello::waos(BUILD)).await {
        Ok(theirs) => theirs,
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            tracing::info!("{error}");
            return None;
        }
        Err(error) => {
            tracing::info!(%error, "a connection ended before it said its hello");
            return None;
        }
    };
    if theirs.major != MAJOR {
        tracing::info!(
            build = %theirs.build,
            major = theirs.major,
            "a connection speaks another protocol's major"
        );
        return None;
    }
    Some(theirs)
}

/// Whether `connection` may be answered `request`: one from this machine always; one from the
/// network a pairing, or anything once its hello carries a paired computer's token.
fn admitted(
    connection: &ServerConnection,
    theirs: &Hello,
    request: &Request,
    access: &DeveloperAccess,
) -> bool {
    match connection.locality() {
        Locality::ThisMachine => true,
        Locality::Network if matches!(request, Request::Pair(_)) => true,
        Locality::Network => theirs
            .token
            .as_ref()
            .is_some_and(|token| access.knows(token)),
    }
}

/// Answers `request` on `connection`.
async fn serve_request(
    mut connection: ServerConnection,
    request: Request,
    context: &ConnectionContext,
) {
    let runs = &context.served.instances;
    match request {
        Request::Install(install) => {
            dev_install::install(connection, install, &context.install()).await;
        }
        Request::Uninstall { id } => {
            dev_install::uninstall(connection, &id, &context.install()).await;
        }
        Request::Launch(launch) => {
            dev_launch::launch(connection, launch, &context.launch()).await;
        }
        Request::Stop { id } => dev_launch::stop(connection, &id, runs).await,
        Request::Attach { id } => {
            let attach = AttachContext {
                runs,
                outputs: context.served.instances.outputs(),
                asking: &context.asking,
            };
            dev_attach::serve(connection, &id, attach).await;
        }
        Request::Extension { id, name, params } => {
            dev_extensions::call(connection, runs, &id, &name, params).await;
        }
        Request::ServingPrograms => {
            dev_extensions::serving(connection, runs, &context.served.packages).await;
        }
        Request::Open { id, files } => {
            if let Some(answer) = dev_refusal::elsewhere(connection.locality(), &id) {
                let _ = connection.send(&answer).await;
                return;
            }
            let request = OpenRequest {
                files,
                links: Vec::new(),
            };
            opening::answer(connection, runs, &context.served.packages, &id, request).await;
        }
        Request::Screenshot { id, window } => {
            dev_screenshot::answer(connection, runs, &context.asking, &id, window).await;
        }
        Request::Input { id, window, input } => {
            dev_input::answer(connection, runs, &context.asking, &id, window, input).await;
        }
        Request::Settings { id } => {
            dev_settings::answer(connection, &context.settings(), &id, None).await;
        }
        Request::Set { id, row } => {
            dev_settings::answer(connection, &context.settings(), &id, Some(row)).await;
        }
        Request::Inspect { id, asked } => {
            let inspection = InspectionContext {
                runs,
                engine: &context.served.engine,
            };
            dev_inspect::answer(connection, inspection, &id, asked).await;
        }
        Request::Resize {
            id,
            window,
            width,
            height,
        } => {
            let size = Size { width, height };
            dev_resize::answer(connection, runs, &id, window, size).await;
        }
        Request::Pair(start) => dev_pairing::pair(connection, start, &context.access).await,
        Request::Unknown => {
            let refusal = dev_refusal::unsupported(BUILD, "request");
            let _ = connection.send(&RefusedAnswer::Refused(refusal)).await;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::Instant;

    use serde::de::DeserializeOwned;
    use waos::{AppInstance, PlatformState, Shell};

    use waos_dev_protocol::{
        Answer, Asked, ClientConnection, Component, DeviceAddress, FileDialog, Fingerprint, Hello,
        Install, InstallAnswer, Launch, LaunchAnswer, OpenAnswer, OutputStream, PairAnswer,
        PairConfirm, PairStart, Pairing, PairingCode, Question, RefusalKind, Request, Row, RunEnd,
        RunEvent, ServingProgramsAnswer, StopAnswer, Token, UninstallAnswer, Upload,
        component_digest, connect_across_network,
    };
    use waos_package::ProgramId;

    use super::*;
    use crate::logic::app_instances::RunningInstances;
    use crate::logic::developer_access::Listening;
    use crate::logic::open_command::{self, Sent};
    use crate::logic::startup::{self, Outside, Setup};
    use crate::logic::stores::Stores;
    use crate::logic::test_engine::{self, NoWindows, REFUSAL, WITHIN};
    use crate::logic::test_installs;
    use waos_services::packages::{
        AppInMemory, CodeCompiler, DevelopmentBuild, InstalledAppsInMemory, InstalledAppsStore,
        InstalledProgram, InstalledVersion, Origin,
    };
    use waos_services::permissions::{NoPrompts, Prompts};

    /// The development id of the program `id`: what `waos install` installs it as, and what a run
    /// of it runs as.
    pub(crate) fn development(id: &str) -> ProgramId {
        id.parse::<ProgramId>().unwrap().development()
    }

    /// The version and category a test's `waos install` sends.
    const VERSION: &str = "0.1.0";
    const CATEGORY: &str = "productivity";

    /// The manifest a test's `waos install` of the program `id` installs.
    fn development_manifest(id: &str) -> waos_package::Manifest {
        waos_package::Manifest::new(id.parse().unwrap(), id, VERSION.parse().unwrap())
            .and_then(|manifest| {
                manifest
                    .with_kind(waos_package::ProgramKind::App)
                    .with_category(CATEGORY.parse().ok())
                    .with_file_associations(Vec::new())
            })
            .unwrap()
    }

    /// Where a test's server keeps its installed apps.
    enum Store {
        InMemory,
        Files,
    }

    /// The development install of `manifest`, as `waos install` installs it, its component the
    /// test program at `component`, its code in `code`.
    fn development_build(
        manifest: waos_package::Manifest,
        component: &str,
        code: PathBuf,
    ) -> AppInMemory {
        let build = DevelopmentBuild {
            manifest,
            component: fs::read(component).unwrap(),
            icon: None,
        };
        let program = InstalledProgram {
            manifest: DevelopmentBuild::installed_manifest(&build.manifest),
            origin: Origin::Development {
                component_sha256: component_digest(&build.component),
            },
        };
        AppInMemory {
            version: Some(InstalledVersion::new(component.into(), code)),
            ..AppInMemory::new(program)
        }
    }

    /// A server of the test's own, over WaOS's one startup: its socket and its apps' own folders
    /// in a folder of its own, removed with it, and its installed apps kept in memory, filled
    /// from the test programs' builds, or, for what an install leaves on disk, on files there.
    pub(crate) struct TestServer {
        pub(crate) folder: PathBuf,
        pub(crate) socket: PathBuf,
        pub(crate) instances: Arc<AppInstances>,
        pub(crate) permissions: waos_services::permissions::Permissions,
        /// What the server's programs' users are asked, which a test notes as the prompts do.
        pub(crate) asking: Arc<AskingNow>,
        pub(crate) packages: Arc<waos_services::packages::PackageManager>,
        /// The installed apps, kept in memory, which the test fills with the apps its build made;
        /// none for a server whose store is on files.
        installed: Option<Arc<InstalledAppsInMemory>>,
        /// The package manager's compiler, which a test holds to see what happens while it
        /// compiles.
        pub(crate) compiler: Arc<test_installs::HeldCompiler>,
        /// Its developer access, off until a test turns it on, listening on the loopback.
        pub(crate) access: DeveloperAccess,
        server: Option<DevServer>,
        /// The headless shell its programs draw on, where they draw.
        headless: Option<waos_headless::HeadlessShell>,
        /// Where its WaOS's own tasks run, as the app's library runtime holds them: not the test's
        /// own, which a test blocks as it installs.
        runtime: Option<tokio::runtime::Runtime>,
    }

    impl TestServer {
        /// A server whose programs run with no windows, its store in memory.
        pub(crate) fn start(test: &str) -> TestServer {
            TestServer::over(test, Store::InMemory, || {
                let shell = test_engine::shell(Arc::new(NoWindows));
                (shell, test_engine::no_screens())
            })
        }

        /// A server whose programs run with no windows, its store on files in its folder: for
        /// what an install leaves on disk, its code compiled and its folder.
        pub(crate) fn on_files(test: &str) -> TestServer {
            TestServer::over(test, Store::Files, || {
                let shell = test_engine::shell(Arc::new(NoWindows));
                (shell, test_engine::no_screens())
            })
        }

        /// A server whose programs' windows draw.
        #[cfg(any(target_vendor = "apple", target_os = "linux"))]
        pub(crate) fn drawing(test: &str) -> TestServer {
            TestServer::drawing_over(test, test_engine::drawing::outside(true))
        }

        /// A server whose programs' views draw in no window, as a phone's deck shows a tab.
        #[cfg(any(target_vendor = "apple", target_os = "linux"))]
        pub(crate) fn drawing_in_no_window(test: &str) -> TestServer {
            TestServer::drawing_over(test, test_engine::drawing::outside(false))
        }

        /// A server over `headless`, whose views it keeps for the test to wait on.
        #[cfg(any(target_vendor = "apple", target_os = "linux"))]
        fn drawing_over(
            test: &str,
            (headless, platform): (waos_headless::HeadlessShell, PlatformState),
        ) -> TestServer {
            let shell = headless.shell();
            let mut server = TestServer::over(test, Store::InMemory, || (shell, platform));
            server.headless = Some(headless);
            server
        }

        /// The pictures the server answers `screenshot` with once they are as `wanted` says,
        /// asked again each time a view of the server's headless shell shows a new one; fails
        /// after [`WITHIN`] with `what` was awaited.
        pub(crate) async fn screenshot_once(
            &self,
            screenshot: &Request,
            what: &str,
            wanted: impl Fn(&[waos_dev_protocol::WindowPicture]) -> bool,
        ) -> Vec<waos_dev_protocol::WindowPicture> {
            let headless = self
                .headless
                .as_ref()
                .expect("a server whose programs draw");
            let shown = || {
                headless
                    .views()
                    .iter()
                    .map(|view| view.pictures_shown())
                    .sum::<usize>()
            };
            let answered = async {
                loop {
                    let before = shown();
                    if let waos_dev_protocol::ScreenshotAnswer::Pictures(pictures) = self
                        .ask::<waos_dev_protocol::ScreenshotAnswer>(screenshot)
                        .await
                        && wanted(&pictures)
                    {
                        return pictures;
                    }
                    headless.changes().until(what, || shown() > before).await;
                }
            };
            tokio::time::timeout(WITHIN, answered)
                .await
                .unwrap_or_else(|_| panic!("never within {WITHIN:?}: {what}"))
        }

        /// A server of the test's own, its store as `store` says, over the shell `outside` makes,
        /// with nobody to ask.
        fn over(
            test: &str,
            store: Store,
            outside: impl FnOnce() -> (Shell, PlatformState),
        ) -> TestServer {
            let folder =
                std::env::temp_dir().join(format!("waos-app-{}-{test}", std::process::id()));
            let _ = fs::remove_dir_all(&folder);
            let socket = Self::test_listener(&folder, test);
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("the server's runtime");
            let (stores, installed) = match store {
                Store::InMemory => {
                    let installed = Arc::new(InstalledAppsInMemory::default());
                    let stores = Stores {
                        installed_apps: Arc::clone(&installed) as Arc<dyn InstalledAppsStore>,
                        ..Stores::in_memory(Vec::new())
                    };
                    (stores, Some(installed))
                }
                Store::Files => (Stores::files(&folder.join("store")).unwrap(), None),
            };
            let compiler = Arc::new(test_installs::HeldCompiler::default());
            let setup = Setup {
                install_compiler: Arc::clone(&compiler) as Arc<dyn CodeCompiler>,
                ..Setup::for_tests(&folder, stores, runtime.handle().clone())
            };
            let prompts = || (Arc::new(NoPrompts) as Arc<dyn Prompts>, ());
            let (started, asking) = startup::start(setup, prompts, |before, ()| {
                let (shell, platform) = outside();
                let asking = AskingNow::new(before.permissions.asking());
                let capabilities = waos_local::LocalCapabilities::new();
                Ok(Outside::new(shell, platform, capabilities, asking))
            })
            .expect("WaOS starts");
            let startup::Started {
                instances,
                packages,
                permissions,
                engine,
                environments,
                ..
            } = started;
            let served = ServedDomains {
                instances: Arc::clone(&instances),
                packages: Arc::clone(&packages),
                permissions: permissions.clone(),
                environments,
                engine,
            };
            let access = DeveloperAccess::on_loopback(folder.join("developer"));
            let server = DevServer::start(served, Arc::clone(&asking), &socket, access.clone())
                .expect("the server binds");
            TestServer {
                folder,
                socket,
                instances,
                permissions,
                asking,
                packages,
                installed,
                compiler,
                access,
                server: Some(server),
                headless: None,
                runtime: Some(runtime),
            }
        }

        /// Where the test's server listens, apart from every other test's and from the user's own
        /// WaOS: a socket in the test's folder, or on Windows a pipe of the test's own name.
        #[cfg(unix)]
        fn test_listener(folder: &Path, _test: &str) -> PathBuf {
            folder.join("dev.sock")
        }

        #[cfg(windows)]
        fn test_listener(_folder: &Path, test: &str) -> PathBuf {
            PathBuf::from(format!(
                r"\\.\pipe\waos-app-test-{}-{test}",
                std::process::id()
            ))
        }

        /// `component` installed, launched and attached to as the program `id`, as `waos run`
        /// runs it: the attach's connection, the program running as `id`'s development id. A test
        /// program is installed first with its code compiled (`test_installs`), as `waos install`
        /// would install it, so the install finds it installed as it is.
        pub(crate) async fn run(&self, id: &str, component: &Path) -> ClientConnection {
            self.run_with(id, component, Vec::new(), Vec::new()).await
        }

        /// [`run`](Self::run), with `variables` added to the program's environment.
        pub(crate) async fn run_with_variables(
            &self,
            id: &str,
            component: &Path,
            variables: Vec<(String, String)>,
        ) -> ClientConnection {
            self.run_with(id, component, Vec::new(), variables).await
        }

        /// [`run`](Self::run), with `arguments` as the program's arguments.
        pub(crate) async fn run_with_args(
            &self,
            id: &str,
            component: &Path,
            arguments: Vec<String>,
        ) -> ClientConnection {
            self.run_with(id, component, arguments, Vec::new()).await
        }

        async fn run_with(
            &self,
            id: &str,
            component: &Path,
            arguments: Vec<String>,
            variables: Vec<(String, String)>,
        ) -> ClientConnection {
            if let Some(path) = component.to_str()
                && waos_test_programs::code_folder(path).is_some()
            {
                self.install_compiled(id, path);
            }
            let component = fs::read(component).unwrap();
            let installed = self.install(id, &component).await;
            assert_eq!(
                installed,
                InstallAnswer::Installed {
                    id: development(id)
                }
            );
            let launched = self.launch(&development(id), arguments, variables).await;
            assert_eq!(launched.last(), Some(&LaunchAnswer::Launched));
            self.attach(&development(id)).await
        }

        /// Installs the test program at `component` as the program `id`'s development install,
        /// as [`install`](Self::install) would, with its code compiled already: in the store in
        /// memory, the build's own, or on files, compiled once in a test run.
        pub(crate) fn install_compiled(&self, id: &str, component: &str) {
            let manifest = development_manifest(id);
            let Some(installed) = &self.installed else {
                test_installs::install_development(&self.packages, manifest, component);
                return;
            };
            let code = waos_test_programs::code_of(component);
            installed.put(development_build(manifest, component, code));
        }

        /// Installs the test program at `component` as the published app `id`, named `name`, in
        /// the store in memory, from the test's build.
        pub(crate) fn install_built(&self, component: &str, id: &str, name: &str) {
            let installed = self.installed.as_ref().expect("a store in memory");
            installed.put(test_installs::built_app(component, id, name));
        }

        /// Installs the test program at `component` as the program `id`'s development install,
        /// in the store in memory, with no code of this engine's, as after an update of WaOS
        /// that changed it: its first launch compiles it, into the server's folder.
        pub(crate) fn install_not_compiled(&self, id: &str, component: &str) {
            let installed = self.installed.as_ref().expect("a store in memory");
            let code = self.folder.join("code").join(id);
            installed.put(development_build(development_manifest(id), component, code));
        }

        /// What WaOS answers an install of `component` as the program `id`, sent in one part.
        pub(crate) async fn install(&self, id: &str, component: &[u8]) -> InstallAnswer {
            let digest = component_digest(component);
            self.install_in_parts_of(id, component, component.len().max(1), digest)
                .await
        }

        /// What WaOS answers an install of `component` as the program `id`, filed under
        /// productivity, announced as `digest` and sent in parts of at most `parts` bytes, unless
        /// WaOS holds it.
        pub(crate) async fn install_in_parts_of(
            &self,
            id: &str,
            component: &[u8],
            parts: usize,
            digest: [u8; 32],
        ) -> InstallAnswer {
            let answers = self.install_answers(id, component, parts, digest).await;
            answers.last().cloned().expect("an install's answer")
        }

        /// Every answer WaOS gives an install of `component` as the program `id`, from a `waos`
        /// that reads `Compiling`, sent in parts of `parts` bytes.
        pub(crate) async fn install_answers(
            &self,
            id: &str,
            component: &[u8],
            parts: usize,
            digest: [u8; 32],
        ) -> Vec<InstallAnswer> {
            let mut client = self.client().await;
            client
                .send(&Request::Install(Install {
                    id: id.parse().unwrap(),
                    kind: waos_package::ProgramKind::App,
                    name: id.to_owned(),
                    version: VERSION.parse().unwrap(),
                    icon: None,
                    category: Some(CATEGORY.to_owned()),
                    file_associations: Vec::new(),
                    length: component.len() as u64,
                    digest,
                    compiling_answer: true,
                }))
                .await
                .unwrap();
            if next::<Upload>(&mut client).await == Upload::Send {
                for part in component.chunks(parts) {
                    client
                        .send(&Component {
                            bytes: part.to_vec(),
                        })
                        .await
                        .unwrap();
                }
            }
            let mut answers = Vec::new();
            loop {
                let answer = next::<InstallAnswer>(&mut client).await;
                let last = answer != InstallAnswer::Compiling;
                answers.push(answer);
                if last {
                    return answers;
                }
            }
        }

        /// What WaOS answers a launch of the installed program `id`, with `arguments` and
        /// `variables`, up to `Launched` or its refusal.
        pub(crate) async fn launch(
            &self,
            id: &ProgramId,
            arguments: Vec<String>,
            variables: Vec<(String, String)>,
        ) -> Vec<LaunchAnswer> {
            let mut client = self.client().await;
            client
                .send(&Request::Launch(Launch {
                    id: id.clone(),
                    arguments,
                    variables,
                }))
                .await
                .unwrap();
            let mut answers = Vec::new();
            loop {
                let answer = next::<LaunchAnswer>(&mut client).await;
                let done = !matches!(answer, LaunchAnswer::Compiling);
                answers.push(answer);
                if done {
                    return answers;
                }
            }
        }

        /// An attach to the program `id`, its request sent.
        pub(crate) async fn attach(&self, id: &ProgramId) -> ClientConnection {
            let mut client = self.client().await;
            client
                .send(&Request::Attach { id: id.clone() })
                .await
                .unwrap();
            client
        }

        /// A client connected, its hello and WaOS's exchanged.
        async fn client(&self) -> ClientConnection {
            let mut client = ClientConnection::connect(&self.socket).await.unwrap();
            client.hello(&Hello::command("waos test")).await.unwrap();
            client
        }

        /// What WaOS answers `request`, sent on a connection of its own.
        pub(crate) async fn ask<A: DeserializeOwned>(&self, request: &Request) -> A {
            let mut client = self.client().await;
            client.send(request).await.unwrap();
            next(&mut client).await
        }

        /// A computer paired with the server, connected from the network with its token, its
        /// hello and WaOS's exchanged.
        async fn network_client(&self) -> ClientConnection {
            let token = Token::new("a token of the test's");
            self.access.pair("test", &token).unwrap();
            self.network_client_with(Some(token)).await
        }

        /// A client connected from the network over `wss://`, pinning the server's certificate,
        /// its hello carrying `token`, and exchanged with WaOS's.
        async fn network_client_with(&self, token: Option<Token>) -> ClientConnection {
            let port = self.listening_port().await;
            let address: DeviceAddress = format!("wss://127.0.0.1:{port}").parse().unwrap();
            let fingerprint = self.access.identity().unwrap().fingerprint();
            let (mut client, _) = connect_across_network(&address, Some(fingerprint))
                .await
                .unwrap();
            let mut ours = Hello::command("waos test");
            ours.token = token;
            let theirs = client.hello(&ours).await.unwrap();
            assert_eq!(theirs.major, waos_dev_protocol::MAJOR);
            client
        }

        /// A pairing begun from the network over `wss://` with `code`, unpinned as pairing is: the
        /// connection, the fingerprint the server answered with, this side's half, and the
        /// server's answer.
        async fn pairing_with(
            &self,
            code: &str,
        ) -> (ClientConnection, Fingerprint, Pairing, PairAnswer) {
            let (client, seen, pairing, answer) = self.pairing_over("wss", code).await;
            (client, seen.expect("a certificate"), pairing, answer)
        }

        /// A pairing begun from the loopback at `scheme` with `code`: the connection, the
        /// fingerprint of the certificate it saw over `wss://`, this side's half, and the
        /// server's answer.
        async fn pairing_over(
            &self,
            scheme: &str,
            code: &str,
        ) -> (ClientConnection, Option<Fingerprint>, Pairing, PairAnswer) {
            let port = self.listening_port().await;
            let address: DeviceAddress = format!("{scheme}://127.0.0.1:{port}").parse().unwrap();
            let (mut client, seen) = connect_across_network(&address, None).await.unwrap();
            client.hello(&Hello::command("waos test")).await.unwrap();
            let (pairing, ours) = Pairing::start(&PairingCode::parse(code).unwrap());
            let start = PairStart {
                computer: "test computer".to_owned(),
                spake: ours,
            };
            client.send(&Request::Pair(start)).await.unwrap();
            let answer = next::<PairAnswer>(&mut client).await;
            (client, seen, pairing, answer)
        }

        /// The port the server listens on for the network, developer access turned on.
        async fn listening_port(&self) -> u16 {
            self.access.set_on(true);
            let mut listening = self.access.listening();
            loop {
                match listening.borrow_and_update().clone() {
                    Listening::On { port } => return port,
                    Listening::Failed(why) => panic!("the server cannot listen: {why}"),
                    Listening::Not => {}
                }
                tokio::time::timeout(WITHIN, listening.changed())
                    .await
                    .expect("the server listens in time")
                    .unwrap();
            }
        }

        /// The programs running now, in no order.
        pub(crate) async fn programs(&self) -> Vec<AppInstance> {
            self.instances.instances()
        }

        /// Waits until the runs are exactly `names`.
        pub(crate) async fn wait_for_runs(&self, names: &[&str]) {
            let mut running = self.instances.running();
            let named = |running: &RunningInstances| {
                running
                    .keys()
                    .map(ToString::to_string)
                    .eq(names.iter().copied())
            };
            tokio::time::timeout(WITHIN, running.wait_for(named))
                .await
                .unwrap_or_else(|_| panic!("the runs never became {names:?}"))
                .expect("the runs are kept as long as the server");
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            drop(self.server.take());
            if let Some(runtime) = self.runtime.take() {
                // A test drops its server inside its own runtime, which may not wait for another.
                runtime.shutdown_background();
            }
            let _ = fs::remove_dir_all(&self.folder);
        }
    }

    /// The next frame, within the time a test waits; a connection closed is a failure.
    pub(crate) async fn next<A: DeserializeOwned>(client: &mut ClientConnection) -> A {
        tokio::time::timeout(WITHIN, client.receive::<A>())
            .await
            .expect("a frame in time")
            .expect("a frame, not an error")
            .expect("a frame, not the close")
    }

    /// The first frame of an attach to a program that runs.
    pub(crate) async fn started(client: &mut ClientConnection) {
        assert_eq!(next::<RunEvent>(client).await, RunEvent::Started);
    }

    /// Every frame up to and including the first `wanted` accepts.
    async fn frames_until(
        client: &mut ClientConnection,
        wanted: impl Fn(&RunEvent) -> bool,
    ) -> Vec<RunEvent> {
        let mut frames = Vec::new();
        loop {
            let frame = next::<RunEvent>(client).await;
            let done = wanted(&frame);
            frames.push(frame);
            if done {
                return frames;
            }
        }
    }

    /// What `frames` carry of the program's stdout, in order.
    pub(crate) fn stdout_of(frames: &[RunEvent]) -> String {
        let bytes: Vec<u8> = frames
            .iter()
            .filter_map(|frame| match frame {
                RunEvent::Output {
                    stream: OutputStream::Stdout,
                    bytes,
                } => Some(bytes.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Forever's first two reports, in order.
    const FIRST_REPORTS: &str = "count 1048576\ncount 2097152\n";

    /// The id forever runs as.
    const FOREVER: &str = "dev.waos.examples.forever";

    /// The development id forever runs as, as text.
    const FOREVER_DEVELOPMENT: &str = "dev.waos.examples.forever.development";

    /// The id arguments runs as.
    const ARGUMENTS: &str = "dev.waos.examples.arguments";

    #[tokio::test]
    async fn a_runs_output_reaches_the_attach_in_order_after_started() {
        let server = TestServer::start("output");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        let mut frames = Vec::new();
        while !stdout_of(&frames).contains(FIRST_REPORTS) {
            frames.push(next(&mut client).await);
        }
        assert_eq!(frames[0], RunEvent::Started);
        assert!(
            frames[1..]
                .iter()
                .all(|frame| matches!(frame, RunEvent::Output { .. })),
            "{frames:?}"
        );
        assert!(stdout_of(&frames).starts_with(FIRST_REPORTS));
    }

    #[tokio::test]
    async fn a_launch_then_an_attach_then_a_stop_run_the_program_detached_between() {
        let server = TestServer::start("launch-attach-stop");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        let id = development(FOREVER);
        server.install_compiled(FOREVER, waos_test_programs::FOREVER);
        assert_eq!(
            server.install(FOREVER, &component).await,
            InstallAnswer::Installed { id: id.clone() }
        );
        assert_eq!(
            server.launch(&id, Vec::new(), Vec::new()).await,
            [LaunchAnswer::Launched],
            "compiled at its install"
        );
        server.wait_for_runs(&[FOREVER_DEVELOPMENT]).await;

        let mut first = server.attach(&id).await;
        started(&mut first).await;
        // What it wrote before anything attached comes first, then what it writes.
        let mut frames = Vec::new();
        while !stdout_of(&frames).contains(FIRST_REPORTS) {
            frames.push(next(&mut first).await);
        }
        drop(first);

        let mut again = server.attach(&id).await;
        started(&mut again).await;
        server.wait_for_runs(&[FOREVER_DEVELOPMENT]).await;
        assert_eq!(
            server
                .ask::<StopAnswer>(&Request::Stop { id: id.clone() })
                .await,
            StopAnswer::Stopped
        );
        let frames = frames_until(&mut again, |frame| matches!(frame, RunEvent::Ended(_))).await;
        assert_eq!(frames.last(), Some(&RunEvent::Ended(RunEnd::Stopped)));
        assert_eq!(again.receive::<RunEvent>().await.unwrap(), None, "closed");
        server.wait_for_runs(&[]).await;
        let StopAnswer::Refused(refusal) = server
            .ask::<StopAnswer>(&Request::Stop { id: id.clone() })
            .await
        else {
            panic!("nothing runs to stop");
        };
        assert_eq!(refusal.kind, RefusalKind::NotRunning);
        let mut none = server.attach(&id).await;
        let RunEvent::Refused(refusal) = next::<RunEvent>(&mut none).await else {
            panic!("nothing runs to attach to");
        };
        assert_eq!(refusal.kind, RefusalKind::NotRunning);
    }

    #[tokio::test]
    async fn closing_an_attach_leaves_the_program_running() {
        let server = TestServer::start("closing");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        drop(client);
        let mut again = server.attach(&development(FOREVER)).await;
        started(&mut again).await;
        let mut frames = Vec::new();
        while !stdout_of(&frames).contains("count") {
            frames.push(next(&mut again).await);
        }
        server.wait_for_runs(&[FOREVER_DEVELOPMENT]).await;
    }

    #[tokio::test]
    async fn a_launch_that_compiles_answers_compiling_first() {
        let server = TestServer::start("launch-compiling");
        server.install_not_compiled(FOREVER, waos_test_programs::FOREVER);
        let launched = server
            .launch(&development(FOREVER), Vec::new(), Vec::new())
            .await;
        assert_eq!(launched, [LaunchAnswer::Compiling, LaunchAnswer::Launched]);
    }

    #[tokio::test]
    async fn an_uninstall_answers_uninstalled_and_then_not_installed() {
        let server = TestServer::start("uninstall");
        let id = development(FOREVER);
        server.install_compiled(FOREVER, waos_test_programs::FOREVER);
        let uninstall = Request::Uninstall { id: id.clone() };

        let answer = server.ask::<UninstallAnswer>(&uninstall).await;

        assert_eq!(answer, UninstallAnswer::Uninstalled);
        let UninstallAnswer::Refused(refusal) = server.ask::<UninstallAnswer>(&uninstall).await
        else {
            panic!("nothing is installed to uninstall");
        };
        assert_eq!(refusal.kind, RefusalKind::NotInstalled);
        let refused = server.launch(&id, Vec::new(), Vec::new()).await;
        assert!(
            matches!(&refused[..], [LaunchAnswer::Refused(refusal)] if refusal.kind == RefusalKind::NotInstalled),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn an_install_that_compiles_says_so_first() {
        let server = TestServer::start("install-compiling");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        let digest = component_digest(&component);
        let answers = server
            .install_answers(FOREVER, &component, component.len(), digest)
            .await;
        assert_eq!(
            answers,
            [
                InstallAnswer::Compiling,
                InstallAnswer::Installed {
                    id: development(FOREVER)
                }
            ]
        );
    }

    #[tokio::test]
    async fn a_component_the_development_install_holds_is_answered_have_and_nothing_is_sent() {
        let server = TestServer::start("have");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        server.install_compiled(FOREVER, waos_test_programs::FOREVER);
        let mut again = server.client().await;
        again
            .send(&Request::Install(Install {
                id: FOREVER.parse().unwrap(),
                kind: waos_package::ProgramKind::App,
                name: FOREVER.to_owned(),
                version: "0.1.0".parse().unwrap(),
                icon: None,
                category: None,
                file_associations: Vec::new(),
                length: component.len() as u64,
                digest: component_digest(&component),
                compiling_answer: false,
            }))
            .await
            .unwrap();
        assert_eq!(next::<Upload>(&mut again).await, Upload::Have);
        // Nothing is sent for a component the development install holds.
        assert_eq!(
            next::<InstallAnswer>(&mut again).await,
            InstallAnswer::Installed {
                id: development(FOREVER)
            }
        );
    }

    #[tokio::test]
    async fn a_launch_waiting_for_its_compile_holds_back_no_other_programs_tools() {
        let server = TestServer::start("loading");
        let mut running = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut running).await;
        server.install_not_compiled(OPENER, waos_test_programs::OPENER);
        let held = server.compiler.hold();
        let launching = {
            let (socket, id) = (server.socket.clone(), development(OPENER));
            tokio::spawn(async move {
                let mut client = ClientConnection::connect(&socket).await.unwrap();
                client.hello(&Hello::command("waos test")).await.unwrap();
                client
                    .send(&Request::Launch(Launch {
                        id,
                        arguments: Vec::new(),
                        variables: Vec::new(),
                    }))
                    .await
                    .unwrap();
                assert_eq!(
                    next::<LaunchAnswer>(&mut client).await,
                    LaunchAnswer::Compiling
                );
                next::<LaunchAnswer>(&mut client).await
            })
        };
        let screenshot = Request::Screenshot {
            id: development(FOREVER),
            window: None,
        };
        let answer = tokio::time::timeout(
            WITHIN,
            server.ask::<waos_dev_protocol::ScreenshotAnswer>(&screenshot),
        )
        .await;
        assert_eq!(
            answer,
            Ok(waos_dev_protocol::ScreenshotAnswer::Pictures(Vec::new())),
            "answered while the launch compiles"
        );
        drop(held);
        assert_eq!(launching.await.unwrap(), LaunchAnswer::Launched);
    }

    #[tokio::test]
    async fn a_launchs_arguments_reach_the_program() {
        let server = TestServer::start("arguments");
        let mut client = server
            .run_with_args(
                ARGUMENTS,
                Path::new(waos_test_programs::ARGUMENTS),
                vec!["one".to_owned(), "two words".to_owned()],
            )
            .await;
        started(&mut client).await;
        let frames = frames_until(&mut client, |frame| matches!(frame, RunEvent::Ended(_))).await;
        assert_eq!(
            stdout_of(&frames),
            "argument: dev.waos.examples.arguments.development\nargument: one\nargument: two words\n"
        );
    }

    #[tokio::test]
    async fn an_attach_to_a_program_the_library_opened_hears_its_output_from_then_on() {
        let server = TestServer::start("attach-library");
        server.install_built(waos_test_programs::FOREVER, FOREVER, "Forever");
        let id: ProgramId = FOREVER.parse().unwrap();
        let configuration = crate::logic::app_instances::OpenConfiguration::new(
            crate::logic::app_instances::OpenedFor::User,
        );
        server.instances.open(&id, configuration).await.unwrap();
        let mut client = server.attach(&id).await;
        started(&mut client).await;
        let mut frames = Vec::new();
        while !stdout_of(&frames).contains(FIRST_REPORTS) {
            frames.push(next(&mut client).await);
        }
        assert!(
            stdout_of(&frames).starts_with(FIRST_REPORTS),
            "what it wrote before the attach first, as kept"
        );
        assert_eq!(
            server.ask::<StopAnswer>(&Request::Stop { id }).await,
            StopAnswer::Stopped
        );
        let frames = frames_until(&mut client, |frame| matches!(frame, RunEvent::Ended(_))).await;
        assert_eq!(frames.last(), Some(&RunEvent::Ended(RunEnd::Stopped)));
    }

    #[tokio::test]
    async fn a_programs_settings_are_read_before_its_first_run_and_set_at_any_time() {
        let server = TestServer::start("settings");
        let id = development(FOREVER);
        let before = match server
            .ask::<waos_dev_protocol::SettingsAnswer>(&Request::Settings { id: id.clone() })
            .await
        {
            waos_dev_protocol::SettingsAnswer::Rows(rows) => rows,
            other => panic!("the rows before a run: {other:?}"),
        };
        assert!(
            before.iter().any(|row| row.key == "camera"),
            "a development program's rows are read before its first run"
        );
        assert!(
            !server
                .folder
                .join("store/programs")
                .join(FOREVER_DEVELOPMENT)
                .exists(),
            "reading the rows keeps nothing"
        );
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;

        let set = Request::Set {
            id: id.clone(),
            row: Row::Question(Question::Camera, Some(Answer::Allow)),
        };
        let waos_dev_protocol::SettingsAnswer::Rows(rows) =
            server.ask::<waos_dev_protocol::SettingsAnswer>(&set).await
        else {
            panic!("the rows after the set");
        };
        let camera = rows.iter().find(|row| row.key == "camera").unwrap();
        assert_eq!(
            (camera.values.as_slice(), camera.follows_global),
            (&["allow".to_owned()][..], false)
        );
        let own = server.permissions.app_settings(&id).unwrap();
        assert_eq!(
            own.answers
                .get(waos_services::permissions::Question::Camera),
            &waos_services::permissions::Setting::Own(waos_services::permissions::Answer::Granted),
            "kept as its page keeps it"
        );
        let machine = rows.iter().find(|row| row.key == "machine").unwrap();
        assert_eq!(machine.values, ["ask"]);
        assert!(
            machine
                .offered
                .iter()
                .any(|offered| offered.id == "this-computer")
        );
    }

    #[tokio::test]
    async fn the_settings_of_a_program_installed_from_a_package_are_read_and_set_too() {
        let server = TestServer::start("settings-published");
        server.install_built(waos_test_programs::FOREVER, FOREVER, "Forever");
        let id: ProgramId = FOREVER.parse().unwrap();
        let set = Request::Set {
            id: id.clone(),
            row: Row::Question(Question::Camera, Some(Answer::Deny)),
        };
        let waos_dev_protocol::SettingsAnswer::Rows(rows) =
            server.ask::<waos_dev_protocol::SettingsAnswer>(&set).await
        else {
            panic!("the rows after the set");
        };
        let camera = rows.iter().find(|row| row.key == "camera").unwrap();
        assert_eq!(camera.values, ["deny"]);
        let own = server.permissions.app_settings(&id).unwrap();
        assert_eq!(
            own.answers
                .get(waos_services::permissions::Question::Camera),
            &waos_services::permissions::Setting::Own(waos_services::permissions::Answer::Denied),
            "kept as its page keeps it"
        );
        let summary = Request::Inspect {
            id,
            asked: waos_dev_protocol::Inspection::Summary { days: 7 },
        };
        assert_eq!(
            server
                .ask::<waos_dev_protocol::InspectAnswer>(&summary)
                .await,
            waos_dev_protocol::InspectAnswer::Inspected(Vec::new()),
            "its summary is read though it does not run, with no day kept"
        );
    }

    #[tokio::test]
    async fn the_camera_and_the_microphone_are_set_simulated_the_microphone_with_what_it_records() {
        let server = TestServer::start("set-simulated");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        let id = development(FOREVER);
        for row in [
            Row::Question(Question::Camera, Some(Answer::Simulate)),
            Row::SimulatedMicrophone(Some(waos_dev_protocol::SimulatedSound::Silence)),
        ] {
            let set = Request::Set {
                id: id.clone(),
                row,
            };
            assert!(matches!(
                server.ask::<waos_dev_protocol::SettingsAnswer>(&set).await,
                waos_dev_protocol::SettingsAnswer::Rows(_)
            ));
        }
        let waos_dev_protocol::SettingsAnswer::Rows(rows) = server
            .ask::<waos_dev_protocol::SettingsAnswer>(&Request::Settings { id: id.clone() })
            .await
        else {
            panic!("the rows");
        };
        let value = |key: &str| {
            rows.iter()
                .find(|row| row.key == key)
                .unwrap()
                .values
                .clone()
        };
        assert_eq!(value("camera"), ["simulated"]);
        assert_eq!(value("simulated-microphone"), ["silence"]);
        let own = server.permissions.app_settings(&id).unwrap();
        assert_eq!(
            own.answers
                .get(waos_services::permissions::Question::Camera),
            &waos_services::permissions::Setting::Own(
                waos_services::permissions::Answer::Simulated
            )
        );
        assert_eq!(
            own.simulated_microphone,
            waos_services::permissions::Setting::Own(waos::SimulatedSound::Silence)
        );
    }

    #[tokio::test]
    async fn a_set_answer_that_names_nothing_now_is_refused_and_changes_nothing() {
        let server = TestServer::start("set-refused");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        let id = development(FOREVER);
        let nowhere = server.folder.join("missing").join("out.pdf");
        for row in [
            Row::Machine(Some("build.example.com".to_owned())),
            Row::FileDialog(FileDialog::SaveFile, vec![nowhere]),
        ] {
            let set = Request::Set {
                id: id.clone(),
                row,
            };
            let answer = server.ask::<waos_dev_protocol::SettingsAnswer>(&set).await;
            assert!(
                matches!(answer, waos_dev_protocol::SettingsAnswer::Refused(_)),
                "{answer:?}"
            );
        }
        let own = server.permissions.app_settings(&id).unwrap();
        assert_eq!(
            own.answer_with,
            waos_services::permissions::AnswerWith::default()
        );
    }

    #[tokio::test]
    async fn what_waos_asks_is_told_to_the_run_and_refuses_its_input() {
        let server = TestServer::start("asking");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        let program = server.programs().await.remove(0);
        let camera = Asked::Question(Question::Camera);
        let waiting =
            server
                .asking
                .while_asked(&program, camera.clone(), std::future::pending::<()>());
        let asked = tokio::spawn(waiting);
        let frames = frames_until(&mut client, |frame| matches!(frame, RunEvent::Asking(_))).await;
        assert_eq!(frames.last(), Some(&RunEvent::Asking(camera.clone())));
        let input = Request::Input {
            id: development(FOREVER),
            window: Some(waos_dev_protocol::WindowId(1)),
            input: waos_dev_protocol::Input::Key("enter".to_owned()),
        };
        let waos_dev_protocol::InputAnswer::Refused(refusal) =
            server.ask::<waos_dev_protocol::InputAnswer>(&input).await
        else {
            panic!("the input refused");
        };
        assert_eq!(refusal.kind, waos_dev_protocol::RefusalKind::Asking(camera));
        asked.abort();
    }

    #[tokio::test]
    async fn a_program_that_returns_before_anything_attaches_tells_the_attach_what_it_wrote() {
        let server = TestServer::start("survey");
        let mut client = server
            .run(
                "dev.waos.examples.survey",
                Path::new(waos_test_programs::SURVEY),
            )
            .await;
        started(&mut client).await;
        let frames = frames_until(&mut client, |frame| matches!(frame, RunEvent::Ended(_))).await;
        // Survey asks for a view, is refused with the reason, and returns the failure.
        assert_eq!(
            frames.last(),
            Some(&RunEvent::Ended(RunEnd::Returned { success: false }))
        );
        let stdout = stdout_of(&frames);
        assert!(stdout.starts_with("system: "), "{stdout}");
        let said = frames
            .iter()
            .filter_map(|frame| match frame {
                RunEvent::Output { bytes, .. } => Some(String::from_utf8_lossy(bytes)),
                _ => None,
            })
            .collect::<String>();
        assert!(said.contains(REFUSAL), "{said}");
        assert_eq!(client.receive::<RunEvent>().await.unwrap(), None, "closed");
        let mut later = server
            .attach(&development("dev.waos.examples.survey"))
            .await;
        let RunEvent::Refused(refusal) = next::<RunEvent>(&mut later).await else {
            panic!("its end was heard once");
        };
        assert_eq!(refusal.kind, RefusalKind::NotRunning);
    }

    #[tokio::test]
    async fn the_variables_a_launch_adds_reach_the_program_and_waos_s_own_win() {
        let server = TestServer::start("variables");
        let mut client = server
            .run_with_variables(
                "dev.waos.examples.environment",
                Path::new(waos_test_programs::ENVIRONMENT),
                vec![
                    ("RUST_LOG".to_owned(), "debug".to_owned()),
                    ("TMPDIR".to_owned(), "/elsewhere".to_owned()),
                ],
            )
            .await;
        started(&mut client).await;
        let frames = frames_until(&mut client, |frame| matches!(frame, RunEvent::Ended(_))).await;
        assert_eq!(
            frames.last(),
            Some(&RunEvent::Ended(RunEnd::Returned { success: true }))
        );
        assert_eq!(
            stdout_of(&frames),
            "RUST_LOG=debug\nTMPDIR=/tmp\n",
            "the launch's variable, and WaOS's own TMPDIR over one of that name"
        );
        assert_eq!(client.receive::<RunEvent>().await.unwrap(), None, "closed");
    }

    #[tokio::test]
    async fn a_component_sent_in_parts_is_assembled_and_runs() {
        let server = TestServer::on_files("parts");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        // Parts far under a frame's worth, so the component spans many of them.
        let installed = server
            .install_in_parts_of(FOREVER, &component, 100_000, component_digest(&component))
            .await;
        assert!(matches!(installed, InstallAnswer::Installed { .. }));
        server
            .launch(&development(FOREVER), Vec::new(), Vec::new())
            .await;
        server.wait_for_runs(&[FOREVER_DEVELOPMENT]).await;
    }

    #[tokio::test]
    async fn a_component_whose_digest_is_not_the_one_announced_is_refused() {
        let server = TestServer::start("wrong-digest");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        let answer = server
            .install_in_parts_of(FOREVER, &component, 100_000, [0xab; 32])
            .await;
        let InstallAnswer::Refused(refusal) = answer else {
            panic!("refused");
        };
        assert!(refusal.message.contains("digest"), "{refusal:?}");
        assert!(server.packages.installed().is_empty());
    }

    #[tokio::test]
    async fn a_client_of_another_protocol_major_is_refused() {
        let server = TestServer::start("protocol");
        let mut client = ClientConnection::connect(&server.socket).await.unwrap();
        let mut newer = Hello::command("waos 0.4.0");
        newer.major = waos_dev_protocol::MAJOR + 1;
        let theirs = client.hello(&newer).await.unwrap();
        assert_eq!(theirs.major, waos_dev_protocol::MAJOR);
        assert_eq!(client.receive::<RunEvent>().await.unwrap(), None, "closed");
    }

    /// The id the opener runs as.
    const OPENER: &str = "dev.waos.examples.opener";

    /// A file to open, beneath the server's folder, saying `text`.
    fn file_to_open(server: &TestServer, text: &str) -> PathBuf {
        fs::create_dir_all(&server.folder).unwrap();
        let file = fs::canonicalize(&server.folder).unwrap().join("notes.md");
        fs::write(&file, text).unwrap();
        file
    }

    #[tokio::test]
    async fn files_opened_with_a_running_program_reach_it() {
        let server = TestServer::start("open-running");
        let mut client = server
            .run(OPENER, Path::new(waos_test_programs::OPENER))
            .await;
        let mut frames = Vec::new();
        while !stdout_of(&frames).contains("listening for open requests") {
            frames.push(next(&mut client).await);
        }
        let file = file_to_open(&server, "my notes");
        let id = development(OPENER);
        let sent = open_command::send_to(&server.socket, &id, vec![file.clone()]).await;
        assert_eq!(sent.unwrap(), Sent::Opened);
        let named = waos::machine_paths::in_program(&file).expect("a path a program names");
        let heard = format!("opened file {}: my notes", named.display());
        while !stdout_of(&frames).contains(&heard) {
            frames.push(next(&mut client).await);
        }
    }

    #[tokio::test]
    async fn opening_files_over_a_network_connection_is_refused_local_only() {
        let server = TestServer::start("open-network");
        let file = file_to_open(&server, "my notes");
        let mut client = server.network_client().await;
        client
            .send(&Request::Open {
                id: OPENER.parse().unwrap(),
                files: vec![file],
            })
            .await
            .unwrap();
        let OpenAnswer::Refused(refusal) = next::<OpenAnswer>(&mut client).await else {
            panic!("refused");
        };
        assert_eq!(refusal.kind, RefusalKind::LocalOnly);
        server.wait_for_runs(&[]).await;
    }

    #[tokio::test]
    async fn a_paired_computer_is_answered_from_the_network() {
        let server = TestServer::start("network-paired");
        let mut client = server.network_client().await;
        client.send(&Request::ServingPrograms).await.unwrap();
        let answer = next::<ServingProgramsAnswer>(&mut client).await;
        assert!(
            matches!(answer, ServingProgramsAnswer::Programs(_)),
            "{answer:?}"
        );
    }

    #[tokio::test]
    async fn a_computer_not_paired_is_refused_whatever_it_asks() {
        let server = TestServer::start("network-unpaired");
        server.access.pair("test", &Token::new("paired")).unwrap();
        for token in [None, Some(Token::new("not paired"))] {
            let mut client = server.network_client_with(token).await;
            client.send(&Request::ServingPrograms).await.unwrap();
            let ServingProgramsAnswer::Refused(refusal) = next(&mut client).await else {
                panic!("refused");
            };
            assert_eq!(refusal.kind, RefusalKind::NotPaired);
            assert!(refusal.message.contains("waos pair"), "{}", refusal.message);
        }
    }

    #[tokio::test]
    async fn a_computer_pairs_with_the_code_shown_and_its_token_is_answered_from_then_on() {
        let server = TestServer::start("pairing");
        let code = server.access.show_code().unwrap();
        let (mut client, seen, pairing, answer) = server.pairing_with(code.as_str()).await;
        let PairAnswer::Spake {
            spake,
            confirmation,
            ..
        } = answer
        else {
            panic!("the device's half, not {answer:?}");
        };
        let key = pairing.finish(&spake).unwrap();
        assert!(key.holds_for_device(seen, &confirmation));
        assert_eq!(seen, server.access.identity().unwrap().fingerprint());
        let confirm = PairConfirm {
            confirmation: key.computer_proof(),
        };
        client.send(&confirm).await.unwrap();
        assert_eq!(next::<PairAnswer>(&mut client).await, PairAnswer::Paired);
        assert_eq!(server.access.shown_code(), None, "the code ends");
        let names: Vec<String> = (server.access.paired().into_iter())
            .map(|computer| computer.name)
            .collect();
        assert_eq!(names, ["test computer"]);
        let mut paired = server.network_client_with(Some(key.token())).await;
        paired.send(&Request::ServingPrograms).await.unwrap();
        let answer = next::<ServingProgramsAnswer>(&mut paired).await;
        assert!(
            matches!(answer, ServingProgramsAnswer::Programs(_)),
            "{answer:?}"
        );
    }

    #[tokio::test]
    async fn a_pairing_over_ws_from_this_machine_proves_the_certificate_without_tls() {
        let server = TestServer::start("pairing-ws");
        let code = server.access.show_code().unwrap();
        let (mut client, seen, pairing, answer) = server.pairing_over("ws", code.as_str()).await;
        assert_eq!(seen, None, "no certificate over ws://");
        let PairAnswer::Spake {
            spake,
            fingerprint,
            confirmation,
        } = answer
        else {
            panic!("the device's half, not {answer:?}");
        };
        let key = pairing.finish(&spake).unwrap();
        assert!(key.holds_for_device(fingerprint, &confirmation));
        assert_eq!(fingerprint, server.access.identity().unwrap().fingerprint());
        let confirm = PairConfirm {
            confirmation: key.computer_proof(),
        };
        client.send(&confirm).await.unwrap();
        assert_eq!(next::<PairAnswer>(&mut client).await, PairAnswer::Paired);
        assert!(server.access.knows(&key.token()));
    }

    #[tokio::test]
    async fn a_pairing_with_another_code_is_refused_and_pairs_nothing() {
        let server = TestServer::start("pairing-wrong");
        let shown = server.access.show_code().unwrap();
        let other = if shown.as_str() == "000000" {
            "111111"
        } else {
            "000000"
        };
        let (mut client, seen, pairing, answer) = server.pairing_with(other).await;
        let PairAnswer::Spake {
            spake,
            confirmation,
            ..
        } = answer
        else {
            panic!("the device's half, not {answer:?}");
        };
        let key = pairing.finish(&spake).unwrap();
        assert!(!key.holds_for_device(seen, &confirmation));
        let confirm = PairConfirm {
            confirmation: key.computer_proof(),
        };
        client.send(&confirm).await.unwrap();
        let PairAnswer::Refused(refusal) = next::<PairAnswer>(&mut client).await else {
            panic!("refused");
        };
        assert_eq!(refusal.kind, RefusalKind::WrongCode);
        assert!(server.access.paired().is_empty());
        assert!(!server.access.knows(&key.token()));
    }

    #[tokio::test]
    async fn a_pairing_while_no_code_is_shown_is_refused() {
        let server = TestServer::start("pairing-closed");
        let (_client, _, _, answer) = server.pairing_with("123456").await;
        let PairAnswer::Refused(refusal) = answer else {
            panic!("refused, not {answer:?}");
        };
        assert_eq!(refusal.kind, RefusalKind::PairingClosed);
    }

    /// An attach from the network, by a paired computer, to a program running on `server`.
    async fn attached_from_the_network(server: &TestServer) -> ClientConnection {
        let mut local = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut local).await;
        let mut client = server.network_client().await;
        let id = development(FOREVER);
        client.send(&Request::Attach { id }).await.unwrap();
        next::<RunEvent>(&mut client).await;
        client
    }

    /// Whether `client`'s connection ends within the time a test waits, whatever it held.
    async fn ends(client: &mut ClientConnection) -> bool {
        let deadline = Instant::now() + WITHIN;
        while Instant::now() < deadline {
            match tokio::time::timeout(WITHIN, client.receive::<RunEvent>()).await {
                Ok(Ok(None) | Err(_)) => return true,
                Ok(Ok(Some(_))) => {}
                Err(_) => return false,
            }
        }
        false
    }

    #[tokio::test]
    async fn a_removed_computers_open_attach_ends_and_the_program_runs_on() {
        let server = TestServer::start("network-removed");
        let mut client = attached_from_the_network(&server).await;
        server.access.remove("test").unwrap();
        assert!(ends(&mut client).await, "the attach ended");
        server.wait_for_runs(&[FOREVER_DEVELOPMENT]).await;
    }

    #[tokio::test]
    async fn turning_developer_access_off_ends_the_open_connections_from_the_network() {
        let server = TestServer::start("network-off-open");
        let mut client = attached_from_the_network(&server).await;
        server.access.set_on(false);
        assert!(ends(&mut client).await, "the attach ended");
    }

    #[tokio::test]
    async fn turning_developer_access_off_stops_listening_for_the_network() {
        let server = TestServer::start("network-off");
        let port = server.listening_port().await;
        server.access.set_on(false);
        let mut listening = server.access.listening();
        while *listening.borrow_and_update() != Listening::Not {
            tokio::time::timeout(WITHIN, listening.changed())
                .await
                .expect("the server stops listening in time")
                .unwrap();
        }
        let address: DeviceAddress = format!("ws://127.0.0.1:{port}").parse().unwrap();
        assert!(connect_across_network(&address, None).await.is_err());
    }

    #[tokio::test]
    async fn stopping_the_server_stops_every_run_and_removes_the_socket() {
        let mut server = TestServer::start("stopping");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        let socket = server.socket.clone();
        let stopping = server.server.take().expect("the server runs");
        tokio::task::spawn_blocking(move || drop(stopping))
            .await
            .unwrap();
        let frames = frames_until(&mut client, |frame| matches!(frame, RunEvent::Ended(_))).await;
        assert_eq!(frames.last(), Some(&RunEvent::Ended(RunEnd::Stopped)));
        assert!(!socket.exists(), "the socket file is gone");
    }

    #[tokio::test]
    async fn an_unknown_request_or_inner_variant_is_refused_naming_this_build_and_leaves_the_connection_whole()
     {
        #[derive(serde::Serialize)]
        #[serde(tag = "kind", content = "data")]
        enum FutureOuter {
            NewCommand { x: u32 },
        }

        #[derive(serde::Serialize)]
        #[serde(tag = "kind", content = "data")]
        enum FutureRequestInner {
            Input {
                id: ProgramId,
                window: Option<waos_dev_protocol::WindowId>,
                input: FutureVariant,
            },
            Set {
                id: ProgramId,
                row: FutureVariant,
            },
            Inspect {
                id: ProgramId,
                asked: FutureVariant,
            },
            InspectTrace {
                id: ProgramId,
                asked: FutureInspect,
            },
        }

        #[derive(serde::Serialize)]
        #[serde(tag = "kind", content = "data")]
        enum FutureInspect {
            Trace(Vec<FutureVariant>),
        }

        #[derive(serde::Serialize)]
        #[serde(tag = "kind", content = "data")]
        enum FutureVariant {
            BrandNew { detail: String },
        }

        let server = TestServer::start("unknown-requests");
        let id = development(FOREVER);

        let assert_unsupported = |refusal: waos_dev_protocol::Refusal| {
            assert_eq!(refusal.kind, RefusalKind::NotServed);
            assert!(refusal.message.contains(BUILD), "{}", refusal.message);
            assert!(
                refusal.message.contains("https://waos.app"),
                "{}",
                refusal.message
            );
        };

        let mut client = ClientConnection::connect(&server.socket).await.unwrap();
        client.hello(&Hello::command("waos 0.9.0")).await.unwrap();
        client
            .send(&FutureOuter::NewCommand { x: 1 })
            .await
            .unwrap();
        let OpenAnswer::Refused(refusal) = next::<OpenAnswer>(&mut client).await else {
            panic!("refusal");
        };
        assert_unsupported(refusal);

        let future = || FutureVariant::BrandNew {
            detail: "v9".to_owned(),
        };
        let mut client = ClientConnection::connect(&server.socket).await.unwrap();
        client.hello(&Hello::command("waos 0.9.0")).await.unwrap();
        client
            .send(&FutureRequestInner::Input {
                id: id.clone(),
                window: None,
                input: future(),
            })
            .await
            .unwrap();
        let waos_dev_protocol::InputAnswer::Refused(refusal) =
            next::<waos_dev_protocol::InputAnswer>(&mut client).await
        else {
            panic!("refusal");
        };
        assert_unsupported(refusal);

        let mut client = ClientConnection::connect(&server.socket).await.unwrap();
        client.hello(&Hello::command("waos 0.9.0")).await.unwrap();
        client
            .send(&FutureRequestInner::Set {
                id: id.clone(),
                row: future(),
            })
            .await
            .unwrap();
        let waos_dev_protocol::SettingsAnswer::Refused(refusal) =
            next::<waos_dev_protocol::SettingsAnswer>(&mut client).await
        else {
            panic!("refusal");
        };
        assert_unsupported(refusal);

        let mut client = ClientConnection::connect(&server.socket).await.unwrap();
        client.hello(&Hello::command("waos 0.9.0")).await.unwrap();
        client
            .send(&FutureRequestInner::Inspect {
                id: id.clone(),
                asked: future(),
            })
            .await
            .unwrap();
        let waos_dev_protocol::InspectAnswer::Refused(refusal) =
            next::<waos_dev_protocol::InspectAnswer>(&mut client).await
        else {
            panic!("refusal");
        };
        assert_unsupported(refusal);

        let mut client = ClientConnection::connect(&server.socket).await.unwrap();
        client.hello(&Hello::command("waos 0.9.0")).await.unwrap();
        client
            .send(&FutureRequestInner::InspectTrace {
                id,
                asked: FutureInspect::Trace(vec![future()]),
            })
            .await
            .unwrap();
        let waos_dev_protocol::InspectAnswer::Refused(refusal) =
            next::<waos_dev_protocol::InspectAnswer>(&mut client).await
        else {
            panic!("refusal");
        };
        assert_unsupported(refusal);
    }

    // Writes the old client's raw bytes on the Unix socket itself; the framing it checks is the
    // same over Windows's named pipe, which the protocol crate's own tests cover.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_legacy_0_2_client_receives_a_legacy_number_frame() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let server = TestServer::start("legacy-client");
        let mut client = tokio::net::UnixStream::connect(&server.socket)
            .await
            .unwrap();
        let mut legacy_hello = Vec::new();
        legacy_hello.extend_from_slice(&4u32.to_le_bytes());
        legacy_hello.extend_from_slice(&10u32.to_le_bytes());
        client.write_all(&legacy_hello).await.unwrap();

        let mut reply = [0u8; 8];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(u32::from_le_bytes(reply[0..4].try_into().unwrap()), 4);
        assert_eq!(u32::from_le_bytes(reply[4..8].try_into().unwrap()), 11);
        let mut eof = [0u8; 1];
        assert_eq!(client.read(&mut eof).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn an_install_the_store_cannot_make_is_refused_naming_the_program() {
        let server = TestServer::on_files("refuse-install");
        let broken = "dev.waos.examples.broken";
        let blocker = server
            .folder
            .join("store/programs")
            .join(development(broken).as_str());
        fs::create_dir_all(blocker.parent().unwrap()).unwrap();
        fs::write(&blocker, b"not a directory").unwrap();
        let InstallAnswer::Refused(refusal) = server.install(broken, b"\0asm").await else {
            panic!("refused");
        };
        assert_eq!(refusal.kind, RefusalKind::Failed);
        assert!(refusal.message.contains(broken), "{}", refusal.message);
    }
}
