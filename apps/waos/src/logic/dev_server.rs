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
//! token is known.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
use super::installed_runs::InstalledRuns;
use super::opening;
use super::{dev_input, dev_resize, dev_screenshot};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinSet;
use waos::OpenRequest;
use waos::bindings::wasdk::platform::windows::Size;
use waos_dev_protocol::{
    Hello, Incoming, Listener, Locality, MAJOR, RefusedAnswer, Request, ServerConnection,
};
use waos_store::Store;

/// This build, as its hello names it: "WaOS 0.3.0".
pub(crate) const BUILD: &str = concat!("WaOS ", env!("CARGO_PKG_VERSION"));

/// How long the connections may take to say their runs ended, once every program is stopped.
const CLOSING_WAIT: Duration = Duration::from_secs(2);

/// How long a connection from the network may take over its TLS and WebSocket handshakes.
const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);

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
    /// Listens at `socket` for `waos`'s requests on `installed`'s engine: installing what it
    /// sends into `store` as development installs, launching any installed program as its
    /// environments say, among its runs, and telling each attach what `asking` says its program's
    /// user is asked; opening files with a program, installed in `store`, opened as `installed`
    /// opens it, or running; reading and driving any program running; and reading and setting the
    /// settings `store` keeps of a development program. Takes connections from the network too
    /// while `access` is on. Refused, as `Listener::bind` refuses, while another WaOS answers
    /// there.
    pub(crate) fn start(
        installed: InstalledRuns,
        store: Store,
        asking: Arc<AskingNow>,
        socket: &Path,
        access: DeveloperAccess,
    ) -> io::Result<DevServer> {
        let (stop, stopped) = oneshot::channel();
        let (bound, binding) = mpsc::channel();
        let (connections_tx, active_connections) = watch::channel(0usize);
        let context = ServerContext {
            installed,
            store,
            asking,
            socket: socket.to_owned(),
            access,
            installing: Arc::default(),
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
    /// The engine, what programs run with and the runs, with how an installed program opens and
    /// its code.
    installed: InstalledRuns,
    /// Where development builds are installed, where an installed program to launch or open
    /// files with is found, and every program's settings.
    store: Store,
    /// What each program's user is asked now.
    asking: Arc<AskingNow>,
    socket: PathBuf,
    /// Whether computers on the network may connect, and which.
    access: DeveloperAccess,
    /// Held by an install or uninstall, one at a time.
    installing: Arc<tokio::sync::Mutex<()>>,
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
    context.installed.runs.stop_all().await;
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
    installed: InstalledRuns,
    store: Store,
    asking: Arc<AskingNow>,
    access: DeveloperAccess,
    installing: Arc<tokio::sync::Mutex<()>>,
}

fn connection_context(context: &ServerContext) -> ConnectionContext {
    ConnectionContext {
        installed: context.installed.clone(),
        store: context.store.clone(),
        asking: Arc::clone(&context.asking),
        access: context.access.clone(),
        installing: Arc::clone(&context.installing),
    }
}

impl ConnectionContext {
    fn install(&self) -> InstallContext {
        InstallContext {
            installed: self.installed.clone(),
            store: self.store.clone(),
            changing: Arc::clone(&self.installing),
        }
    }

    fn launch(&self) -> LaunchContext {
        LaunchContext {
            installed: self.installed.clone(),
            store: self.store.clone(),
        }
    }

    fn settings(&self) -> SettingsContext {
        SettingsContext {
            store: self.store.clone(),
            environments: Arc::clone(&self.installed.environments),
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
    serve_request(connection, request, &context).await;
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
    let runs = &context.installed.runs;
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
                outputs: &context.installed.outputs,
                asking: &context.asking,
            };
            dev_attach::serve(connection, &id, attach).await;
        }
        Request::Extension { id, name, params } => {
            dev_extensions::call(connection, runs, &id, &name, params).await;
        }
        Request::ServingPrograms => {
            dev_extensions::serving(connection, runs, &context.store).await;
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
            let (installed, store) = (&context.installed, &context.store);
            opening::answer(connection, installed, store, &id, request).await;
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
                engine: &context.installed.engine,
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
    use std::time::{Duration, Instant};

    use serde::de::DeserializeOwned;
    use waos::{RunningProgram, WaOS};

    use waos_dev_protocol::{
        Answer, Asked, ClientConnection, Component, DeviceAddress, FileDialog, Fingerprint, Hello,
        Install, InstallAnswer, Launch, LaunchAnswer, OpenAnswer, OutputStream, PairAnswer,
        PairConfirm, PairStart, Pairing, PairingCode, Question, RefusalKind, Request, Row, RunEnd,
        RunEvent, ServingProgramsAnswer, StopAnswer, Token, UninstallAnswer, Upload,
        component_digest, connect_across_network,
    };
    use waos_package::ProgramId;

    use super::*;
    use crate::logic::developer_access::Listening;
    use crate::logic::environments::ProgramEnvironments;
    use crate::logic::open_command::{self, Sent};
    use crate::logic::opened_package::tests::write_package;
    use crate::logic::program_places::{ProgramPlace, ProgramPlaces};
    use crate::logic::quit_question::every_program_agrees;
    use crate::logic::runs::Runs;
    use crate::logic::test_engine::{self, NoWindows, REFUSAL};

    /// How long a test waits for a program to do something.
    pub(crate) const WITHIN: Duration = Duration::from_secs(60);

    /// The development id of the program `id`: what `waos install` installs it as, and what a run
    /// of it runs as.
    pub(crate) fn development(id: &str) -> ProgramId {
        id.parse::<ProgramId>().unwrap().development()
    }

    /// A server of the test's own: its socket, its store and its code cache in a folder of its
    /// own, removed with it.
    pub(crate) struct TestServer {
        pub(crate) folder: PathBuf,
        pub(crate) socket: PathBuf,
        runs: Arc<Runs>,
        pub(crate) store: waos_store::Store,
        /// What the server's programs' users are asked, which a test notes as the prompts do.
        pub(crate) asking: Arc<AskingNow>,
        installed: InstalledRuns,
        /// Its developer access, off until a test turns it on, listening on the loopback.
        pub(crate) access: DeveloperAccess,
        server: Option<DevServer>,
    }

    impl TestServer {
        /// A server whose programs run with no windows.
        pub(crate) fn start(test: &str) -> TestServer {
            TestServer::over(test, |code| {
                Arc::new(test_engine::engine(Arc::new(NoWindows), code))
            })
        }

        /// A server whose programs' windows draw.
        #[cfg(any(target_vendor = "apple", target_os = "linux"))]
        pub(crate) fn drawing(test: &str) -> TestServer {
            TestServer::over(test, test_engine::drawing::engine)
        }

        /// A server whose programs' views draw in no window, as a phone's deck shows a tab.
        #[cfg(any(target_vendor = "apple", target_os = "linux"))]
        pub(crate) fn drawing_in_no_window(test: &str) -> TestServer {
            TestServer::over(test, test_engine::drawing::deck_engine)
        }

        /// A server whose engine uses `PromptsOnRequest` wired to `asking`, returning the server
        /// and the prompt requests receiver.
        fn asking_prompts(test: &str) -> (TestServer, crate::logic::prompts::PromptRequests) {
            let asking = AskingNow::new();
            let (prompts, requests) =
                crate::logic::prompts::PromptsOnRequest::new(Arc::clone(&asking));
            let server = TestServer::over_with(test, asking, |code| {
                Arc::new(test_engine::engine_asking(
                    Arc::new(NoWindows),
                    Arc::new(prompts),
                    code,
                ))
            });
            (server, requests)
        }

        /// A server of the test's own over the engine `engine` makes, keeping its compiled
        /// programs where it is given.
        fn over(test: &str, engine: impl FnOnce(&Path) -> Arc<WaOS>) -> TestServer {
            Self::over_with(test, AskingNow::new(), engine)
        }

        fn over_with(
            test: &str,
            asking: Arc<AskingNow>,
            engine: impl FnOnce(&Path) -> Arc<WaOS>,
        ) -> TestServer {
            let folder =
                std::env::temp_dir().join(format!("waos-app-{}-{test}", std::process::id()));
            let _ = fs::remove_dir_all(&folder);
            let engine = engine(&folder.join("code"));
            let socket = Self::test_listener(&folder, test);
            let runs = Arc::new(Runs::default());
            let store =
                waos_store::Store::open(&folder.join("store"), waos_store::SourceProxy::None)
                    .expect("a store");
            let installed = InstalledRuns::new(
                engine,
                ProgramEnvironments::new(waos_local::LocalCapabilities::new(), store.clone()),
                Arc::clone(&runs),
                ProgramPlaces::new(ProgramPlace::Windows),
            );
            let access = DeveloperAccess::on_loopback(folder.join("developer"));
            let server = DevServer::start(
                installed.clone(),
                store.clone(),
                Arc::clone(&asking),
                &socket,
                access.clone(),
            )
            .expect("the server binds");
            TestServer {
                folder,
                socket,
                runs,
                store,
                asking,
                installed,
                access,
                server: Some(server),
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
        /// runs it: the attach's connection, the program running as `id`'s development id.
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
            let mut client = self.client().await;
            client
                .send(&Request::Install(Install {
                    id: id.parse().unwrap(),
                    kind: waos_package::ProgramKind::App,
                    name: id.to_owned(),
                    version: "0.1.0".parse().unwrap(),
                    icon: None,
                    category: Some("productivity".to_owned()),
                    file_associations: Vec::new(),
                    length: component.len() as u64,
                    digest,
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
            next(&mut client).await
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

        /// A pairing begun from the network with `code`, unpinned as pairing is: the connection,
        /// the fingerprint the server answered with, this side's half, and the server's answer.
        async fn pairing_with(
            &self,
            code: &str,
        ) -> (ClientConnection, Fingerprint, Pairing, PairAnswer) {
            let port = self.listening_port().await;
            let address: DeviceAddress = format!("wss://127.0.0.1:{port}").parse().unwrap();
            let (mut client, seen) = connect_across_network(&address, None).await.unwrap();
            client.hello(&Hello::command("waos test")).await.unwrap();
            let (pairing, ours) = Pairing::start(&PairingCode::parse(code).unwrap());
            let start = PairStart {
                computer: "test computer".to_owned(),
                spake: ours,
            };
            client.send(&Request::Pair(start)).await.unwrap();
            let answer = next::<PairAnswer>(&mut client).await;
            (client, seen.expect("a certificate"), pairing, answer)
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
        pub(crate) async fn programs(&self) -> Vec<RunningProgram> {
            self.runs.programs().await
        }

        /// Waits until the runs are exactly `names`.
        pub(crate) async fn wait_for_runs(&self, names: &[&str]) {
            let deadline = Instant::now() + WITHIN;
            loop {
                let mut running = self.runs.names().await;
                running.sort();
                if running == names {
                    return;
                }
                assert!(Instant::now() < deadline, "the runs stayed {running:?}");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            drop(self.server.take());
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
        assert_eq!(
            server.install(FOREVER, &component).await,
            InstallAnswer::Installed { id: id.clone() }
        );
        assert_eq!(
            server.launch(&id, Vec::new(), Vec::new()).await,
            [LaunchAnswer::Compiling, LaunchAnswer::Launched]
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
    async fn a_second_run_of_the_same_program_ends_the_first_with_replaced() {
        let server = TestServer::start("replaced");
        let mut first = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut first).await;
        let mut second = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        let frames = frames_until(&mut first, |frame| matches!(frame, RunEvent::Ended(_))).await;
        assert_eq!(frames.last(), Some(&RunEvent::Ended(RunEnd::Replaced)));
        started(&mut second).await;
        server.wait_for_runs(&[FOREVER_DEVELOPMENT]).await;
    }

    #[tokio::test]
    async fn a_component_installed_again_unchanged_launches_from_the_code_compiled_before() {
        let server = TestServer::start("unchanged");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        let id = development(FOREVER);
        server.install(FOREVER, &component).await;
        let first = server.launch(&id, Vec::new(), Vec::new()).await;
        assert_eq!(first, [LaunchAnswer::Compiling, LaunchAnswer::Launched]);
        server
            .ask::<StopAnswer>(&Request::Stop { id: id.clone() })
            .await;
        server.wait_for_runs(&[]).await;
        server.install(FOREVER, &component).await;
        let again = server.launch(&id, Vec::new(), Vec::new()).await;
        assert_eq!(again, [LaunchAnswer::Launched], "nothing compiled again");
    }

    #[tokio::test]
    async fn a_component_the_development_install_holds_is_answered_have_and_nothing_is_sent() {
        let server = TestServer::start("have");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        server.install(FOREVER, &component).await;
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
    async fn a_component_installed_changed_is_compiled_again_and_keeps_the_programs_data() {
        let server = TestServer::start("changed");
        let id = development(FOREVER);
        let mut first = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut first).await;
        let data = server.store.folders(&id).unwrap().data;
        fs::write(data.join("kept.txt"), "kept").unwrap();
        let changed = fs::read(waos_test_programs::OPENER).unwrap();
        assert_eq!(
            server.install(FOREVER, &changed).await,
            InstallAnswer::Installed { id: id.clone() }
        );
        let frames = frames_until(&mut first, |frame| matches!(frame, RunEvent::Ended(_))).await;
        assert_eq!(
            frames.last(),
            Some(&RunEvent::Ended(RunEnd::Replaced)),
            "the install of a new build ends the run of the one before"
        );
        let launched = server.launch(&id, Vec::new(), Vec::new()).await;
        assert_eq!(launched, [LaunchAnswer::Compiling, LaunchAnswer::Launched]);
        assert_eq!(fs::read_to_string(data.join("kept.txt")).unwrap(), "kept");
    }

    #[tokio::test]
    async fn a_launch_waiting_for_its_compile_holds_back_no_other_programs_tools() {
        let server = TestServer::start("loading");
        let mut running = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut running).await;
        let held = server.installed.code.hold();
        let opener = fs::read(waos_test_programs::OPENER).unwrap();
        server.install(OPENER, &opener).await;
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
            Duration::from_secs(5),
            server.ask::<waos_dev_protocol::ScreenshotAnswer>(&screenshot),
        )
        .await;
        assert_eq!(
            answer,
            Ok(waos_dev_protocol::ScreenshotAnswer::Pictures(Vec::new()))
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
    async fn a_run_is_its_development_install_with_its_own_folders_beside_the_published_one() {
        let server = TestServer::start("folders");
        let packages = server.folder.join("packages");
        fs::create_dir_all(&packages).unwrap();
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        let key = waos_package::SigningKey::generate();
        let package = write_package(&packages, FOREVER, "Forever", "1.0.0", &component, &key);
        server
            .store
            .install(waos_package::Package::open(&package).unwrap())
            .unwrap();
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        let program = server
            .folder
            .join("store/programs")
            .join(FOREVER_DEVELOPMENT);
        assert!(program.join("data").is_dir());
        assert!(program.join("cache").is_dir());
        let names = server.runs.programs().await;
        assert_eq!(names[0].name(), FOREVER_DEVELOPMENT);
        let development = server
            .store
            .installed_program(&development(FOREVER))
            .expect("its development install");
        assert!(development.is_development());
        assert_eq!(
            development.manifest.category(),
            Some(waos_package::Category::Productivity),
            "filed as its source says"
        );
        let published = server.store.installed_program(&FOREVER.parse().unwrap());
        assert_eq!(
            published.and_then(|installed| installed.publisher()),
            Some(key.publisher()),
            "the published program keeps its publisher's key"
        );
    }

    #[tokio::test]
    async fn an_uninstall_stops_the_program_and_removes_it_with_its_folders() {
        let server = TestServer::start("uninstall");
        let id = development(FOREVER);
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        assert_eq!(
            server
                .ask::<UninstallAnswer>(&Request::Uninstall { id: id.clone() })
                .await,
            UninstallAnswer::Uninstalled
        );
        let frames = frames_until(&mut client, |frame| matches!(frame, RunEvent::Ended(_))).await;
        assert_eq!(frames.last(), Some(&RunEvent::Ended(RunEnd::Stopped)));
        server.wait_for_runs(&[]).await;
        assert!(server.store.installed().is_empty());
        assert!(
            !server
                .folder
                .join("store/programs")
                .join(FOREVER_DEVELOPMENT)
                .exists()
        );
        let UninstallAnswer::Refused(refusal) = server
            .ask::<UninstallAnswer>(&Request::Uninstall { id: id.clone() })
            .await
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
    async fn an_attach_to_a_program_the_library_opened_hears_its_output_from_then_on() {
        let server = TestServer::start("attach-library");
        let packages = server.folder.join("packages");
        fs::create_dir_all(&packages).unwrap();
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        let key = waos_package::SigningKey::generate();
        let package = write_package(&packages, FOREVER, "Forever", "1.0.0", &component, &key);
        server
            .store
            .install(waos_package::Package::open(&package).unwrap())
            .unwrap();
        let id: ProgramId = FOREVER.parse().unwrap();
        let component = server.store.program(&id).unwrap();
        server
            .installed
            .open(
                &id,
                component,
                ProgramPlace::Windows,
                crate::logic::installed_runs::Opening::ByTheUser,
            )
            .await
            .unwrap();
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
        let own = server.store.program_settings(&id).unwrap();
        assert_eq!(
            own.answers.get(waos_store::Question::Camera),
            &waos_store::Setting::Own(waos_store::Answer::Granted),
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
        let packages = server.folder.join("packages");
        fs::create_dir_all(&packages).unwrap();
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        let key = waos_package::SigningKey::generate();
        let package = write_package(&packages, FOREVER, "Forever", "1.0.0", &component, &key);
        server
            .store
            .install(waos_package::Package::open(&package).unwrap())
            .unwrap();
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
        let own = server.store.program_settings(&id).unwrap();
        assert_eq!(
            own.answers.get(waos_store::Question::Camera),
            &waos_store::Setting::Own(waos_store::Answer::Denied),
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
        let own = server.store.program_settings(&id).unwrap();
        assert_eq!(
            own.answers.get(waos_store::Question::Camera),
            &waos_store::Setting::Own(waos_store::Answer::Simulated)
        );
        assert_eq!(
            own.simulated_microphone,
            waos_store::Setting::Own(waos::SimulatedSound::Silence)
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
        let own = server.store.program_settings(&id).unwrap();
        assert_eq!(own.answer_with, waos_store::AnswerWith::default());
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
    async fn settings_set_answers_a_waiting_question_and_drops_its_prompt() {
        use futures::StreamExt;

        const PERMISSIONS: &str = "dev.waos.examples.permissions";
        let (server, mut requests) = TestServer::asking_prompts("set-answers-waiting");
        let mut client = server
            .run(PERMISSIONS, Path::new(waos_test_programs::PERMISSIONS))
            .await;
        started(&mut client).await;

        let frames = frames_until(&mut client, |frame| matches!(frame, RunEvent::Asking(_))).await;
        assert_eq!(
            frames.last(),
            Some(&RunEvent::Asking(Asked::Question(Question::Notifications)))
        );
        let pending_question = requests.questions.next().await.expect("waiting question");
        assert_eq!(
            server.asking.of(development(PERMISSIONS).as_str()),
            Some(Asked::Question(Question::Notifications))
        );

        let set = Request::Set {
            id: development(PERMISSIONS),
            row: Row::Question(Question::Notifications, Some(Answer::Allow)),
        };
        assert!(matches!(
            server.ask::<waos_dev_protocol::SettingsAnswer>(&set).await,
            waos_dev_protocol::SettingsAnswer::Rows(_)
        ));

        let mut output = String::new();
        while !output.contains("notifications asked: granted") {
            let frame: RunEvent = next(&mut client).await;
            if let RunEvent::Output {
                stream: OutputStream::Stdout,
                bytes,
            } = frame
            {
                output.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        assert!(pending_question.cancelled.await.is_err());
        assert_ne!(
            server.asking.of(development(PERMISSIONS).as_str()),
            Some(Asked::Question(Question::Notifications))
        );
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
    async fn a_program_that_does_not_listen_agrees_to_end_when_asked() {
        let server = TestServer::start("asked");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        let programs = server.runs.programs().await;
        assert_eq!(programs.len(), 1);
        let asked = programs.iter().map(RunningProgram::request_exit);
        let agreed = tokio::time::timeout(WITHIN, every_program_agrees(asked)).await;
        assert_eq!(agreed, Ok(true));
    }

    #[tokio::test]
    async fn a_component_that_is_not_one_is_installed_and_its_launch_refused() {
        let server = TestServer::start("refused");
        let nonsense = "dev.waos.examples.nonsense";
        let installed = server
            .install_in_parts_of(
                nonsense,
                b"not a component",
                4,
                component_digest(b"not a component"),
            )
            .await;
        assert_eq!(
            installed,
            InstallAnswer::Installed {
                id: development(nonsense)
            }
        );
        let answers = server
            .launch(&development(nonsense), Vec::new(), Vec::new())
            .await;
        let Some(LaunchAnswer::Refused(refusal)) = answers.last() else {
            panic!("refused: {answers:?}");
        };
        assert_eq!(refusal.kind, RefusalKind::CannotLoad);
        assert!(refusal.message.contains("nonsense"), "{refusal:?}");
        server.wait_for_runs(&[]).await;
    }

    #[tokio::test]
    async fn a_component_sent_in_parts_is_assembled_and_runs() {
        let server = TestServer::start("parts");
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
        assert!(server.store.installed().is_empty());
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
    async fn files_opened_with_an_installed_program_start_it() {
        let server = TestServer::start("open-installed");
        let packages = server.folder.join("packages");
        fs::create_dir_all(&packages).unwrap();
        let component = fs::read(waos_test_programs::OPENER).unwrap();
        let key = waos_package::SigningKey::generate();
        let package = write_package(&packages, OPENER, "Opener", "1.0.0", &component, &key);
        server
            .store
            .install(waos_package::Package::open(&package).unwrap())
            .unwrap();
        let file = file_to_open(&server, "my notes");
        let id = OPENER.parse().unwrap();
        let sent = open_command::send_to(&server.socket, &id, vec![file.clone()]).await;
        assert_eq!(sent.unwrap(), Sent::Opened);
        server.wait_for_runs(&[OPENER]).await;
        let deadline = Instant::now() + WITHIN;
        while fs::read_to_string(&file).unwrap() != "my notes, seen" {
            assert!(Instant::now() < deadline, "the program never wrote to it");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn files_opened_with_a_program_neither_installed_nor_running_are_refused() {
        let server = TestServer::start("open-nothing");
        let file = file_to_open(&server, "my notes");
        let id = OPENER.parse().unwrap();
        let refused = open_command::send_to(&server.socket, &id, vec![file])
            .await
            .unwrap_err();
        assert!(
            format!("{refused:#}").contains("neither installed nor running"),
            "{refused:#}"
        );
        server.wait_for_runs(&[]).await;
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
    async fn a_program_writing_while_nobody_reads_keeps_running_and_is_stopped_in_time() {
        const FLOOD: &str = "dev.waos.examples.flood";
        let server = TestServer::start("flood");
        let mut first = server
            .run(FLOOD, Path::new(waos_test_programs::FLOOD))
            .await;
        started(&mut first).await;

        let data = server
            .folder
            .join("store/programs")
            .join(development(FLOOD).as_str())
            .join("data");
        let read_u64 = |name: &str| -> u64 {
            fs::read_to_string(data.join(name))
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0)
        };
        let deadline = Instant::now() + WITHIN;
        while read_u64("wrote") < (1 << 20) || read_u64("ticks") == 0 {
            assert!(Instant::now() < deadline, "flood never filled the queue");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let (wrote_before, ticks_before) = loop {
            let wrote_a = read_u64("wrote");
            let ticks_a = read_u64("ticks");
            tokio::time::sleep(Duration::from_millis(150)).await;
            let wrote_b = read_u64("wrote");
            let ticks_b = read_u64("ticks");
            if wrote_b == wrote_a {
                break (wrote_a, (ticks_a, ticks_b));
            }
            assert!(
                Instant::now() < deadline,
                "output never stalled on full backpressure ({wrote_a} -> {wrote_b})"
            );
        };
        assert!(wrote_before >= (1 << 20));
        assert!(
            ticks_before.1 > ticks_before.0,
            "timer kept running while stdout waited ({} -> {})",
            ticks_before.0,
            ticks_before.1
        );

        let frames = tokio::time::timeout(Duration::from_secs(1), async {
            let mut replacing = server
                .run(FLOOD, Path::new(waos_test_programs::FLOOD))
                .await;
            started(&mut replacing).await;
            let frames =
                frames_until(&mut first, |frame| matches!(frame, RunEvent::Ended(_))).await;
            drop(replacing);
            frames
        })
        .await
        .expect("a replacing launch stops the blocked writer within a second");
        assert_eq!(frames.last(), Some(&RunEvent::Ended(RunEnd::Replaced)));
        assert!(stdout_of(&frames).len() >= (1 << 20));
    }

    #[tokio::test]
    async fn a_program_flooding_println_while_nobody_reads_is_stopped_in_time() {
        const FLOOD: &str = "dev.waos.examples.flood";
        let server = TestServer::start("flood-println");
        let mut first = server
            .run_with_args(
                FLOOD,
                Path::new(waos_test_programs::FLOOD),
                vec!["--println".to_owned()],
            )
            .await;
        started(&mut first).await;

        let wrote_file = server
            .folder
            .join("store/programs")
            .join(development(FLOOD).as_str())
            .join("data/wrote");
        let read_wrote = || -> u64 {
            fs::read_to_string(&wrote_file)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0)
        };
        let deadline = Instant::now() + WITHIN;
        while read_wrote() < (1 << 20) {
            assert!(Instant::now() < deadline, "println never filled the queue");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        loop {
            let wrote_a = read_wrote();
            tokio::time::sleep(Duration::from_millis(150)).await;
            let wrote_b = read_wrote();
            if wrote_b == wrote_a {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "println never stalled on full backpressure ({wrote_a} -> {wrote_b})"
            );
        }

        let frames = tokio::time::timeout(Duration::from_secs(1), async {
            let mut replacing = server
                .run(FLOOD, Path::new(waos_test_programs::FLOOD))
                .await;
            started(&mut replacing).await;
            let frames =
                frames_until(&mut first, |frame| matches!(frame, RunEvent::Ended(_))).await;
            drop(replacing);
            frames
        })
        .await
        .expect("a replacing launch stops the println writer within a second");
        assert_eq!(frames.last(), Some(&RunEvent::Ended(RunEnd::Replaced)));
        assert!(stdout_of(&frames).len() >= (1 << 20));
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
        let server = TestServer::start("refuse-install");
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
