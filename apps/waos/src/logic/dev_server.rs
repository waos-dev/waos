//! Where `cargo waos` and `waos open` reach this WaOS: a listener on the socket `waos-dev-protocol`
//! names, on a thread of the app's own with a runtime of its own, and a task per connection,
//! each, once the hellos are exchanged, one request: a run (`dev_run`), one opening of files with
//! a program (`opening`), one screenshot of the windows of a program a run runs
//! (`dev_screenshot`), one input to one of them (`dev_input`), a size for one of them
//! (`dev_resize`), a program's settings read or one of them set (`dev_settings`), or an inspection
//! of a program (`dev_inspect`). A connection of another protocol's major, or with a hello this
//! build cannot read, ends before it asks anything. Stopping the server stops every run, waits
//! for each connection to say so, and removes the socket file.
//!
//! The invariant: the server's thread owns the listener, so the socket file goes with the
//! thread, and dropping the server returns only once the thread has: a WaOS that quit leaves no
//! socket file behind.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::asking_now::AskingNow;
use super::dev_compiles::DevCompiles;
use super::dev_inspect::{self, InspectionContext};
use super::dev_refusal;
use super::dev_run::{self, RunContext};
use super::dev_settings::{self, SettingsContext};
use super::installed_runs::InstalledRuns;
use super::opening;
use super::sent_components::SentComponents;
use super::{dev_input, dev_resize, dev_screenshot};
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use waos::OpenRequest;
use waos::bindings::wasdk::platform::windows::Size;
use waos_dev_protocol::{Connection, Hello, Listener, MAJOR, Request, ServerStream};
use waos_store::Store;

/// This build, as its hello names it: "WaOS 0.3.0".
const BUILD: &str = concat!("WaOS ", env!("CARGO_PKG_VERSION"));

/// How long the connections may take to say their runs ended, once every program is stopped.
const CLOSING_WAIT: Duration = Duration::from_secs(2);

/// The server, listening until dropped. Dropping it stops every run, waits for the connections
/// to hear so, removes the socket file, and returns once all that is done.
pub(crate) struct DevServer {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    /// The components sent, whose writes a test holds back.
    #[cfg(test)]
    components: Arc<SentComponents>,
}

impl DevServer {
    /// Listens at `socket` for runs on `installed`'s engine, each program run as its environments
    /// say, keeping each component sent under `components` and each program running among its
    /// runs, telling each run what `asking` says its program's user is asked and reporting each
    /// compile it makes to `dev_compiles`; for files to
    /// open with a program, installed in `store`, opened as `installed` opens it, or running; and
    /// for the settings `store` keeps of a program a run has run. Refused, as `Listener::bind`
    /// refuses, while another WaOS answers there.
    pub(crate) fn start(
        installed: InstalledRuns,
        store: Store,
        asking: Arc<AskingNow>,
        socket: &Path,
        components: Arc<SentComponents>,
        dev_compiles: Option<Arc<DevCompiles>>,
    ) -> io::Result<DevServer> {
        let (stop, stopped) = oneshot::channel();
        let (bound, binding) = mpsc::channel();
        let context = ServerContext {
            installed,
            store,
            asking,
            socket: socket.to_owned(),
            components: Arc::clone(&components),
            dev_compiles,
        };
        let thread = thread::Builder::new()
            .name("dev server".into())
            .spawn(move || serve_on_this_thread(context, &bound, stopped))?;
        match binding.recv() {
            Ok(Ok(())) => Ok(DevServer {
                stop: Some(stop),
                thread: Some(thread),
                #[cfg(test)]
                components,
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
    /// The engine, what programs run with and the runs, with how an installed program opens.
    installed: InstalledRuns,
    /// Where an installed program to open files with is found, and every program's settings.
    store: Store,
    /// What each program's user is asked now.
    asking: Arc<AskingNow>,
    socket: PathBuf,
    components: Arc<SentComponents>,
    /// Which programs' code the runs' compiles are making, for the activity that keeps macOS from
    /// throttling WaOS; none where nothing follows them.
    dev_compiles: Option<Arc<DevCompiles>>,
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
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(connection) => {
                    connections.spawn(serve_connection(connection, connection_context(&context)));
                }
                Err(error) => {
                    tracing::warn!(%error, "accepting a connection");
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

/// What one connection is served with: a run's, an opening's, or a program's settings'.
struct ConnectionContext {
    run: RunContext,
    installed: InstalledRuns,
    store: Store,
    settings: SettingsContext,
}

fn connection_context(context: &ServerContext) -> ConnectionContext {
    ConnectionContext {
        run: RunContext {
            engine: Arc::clone(&context.installed.engine),
            environments: Arc::clone(&context.installed.environments),
            runs: Arc::clone(&context.installed.runs),
            components: Arc::clone(&context.components),
            dev_compiles: context.dev_compiles.clone(),
            asking: Arc::clone(&context.asking),
        },
        installed: context.installed.clone(),
        store: context.store.clone(),
        settings: SettingsContext {
            runs: Arc::clone(&context.installed.runs),
            components: Arc::clone(&context.components),
            store: context.store.clone(),
            environments: Arc::clone(&context.installed.environments),
        },
    }
}

/// Serves `connection` to its end: the request its first message asks for, or nothing when it
/// asks for none, after the hellos say the two halves speak the same protocol.
async fn serve_connection(mut connection: Connection<ServerStream>, context: ConnectionContext) {
    let theirs = match connection.hello(&Hello::waos(BUILD)).await {
        Ok(theirs) => theirs,
        Err(error) => {
            tracing::info!(%error, "a connection ended before it said its hello");
            return;
        }
    };
    if theirs.major != MAJOR {
        tracing::info!(
            build = %theirs.build,
            major = theirs.major,
            "a connection speaks another protocol's major"
        );
        return;
    }
    match connection.receive::<Request>().await {
        Ok(Some(Request::Run(run))) => {
            dev_run::serve(connection, run, context.run).await;
        }
        Ok(Some(Request::Open { id, files })) => {
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
        Ok(Some(Request::Screenshot { id, window })) => {
            let (runs, asking) = (&context.run.runs, &context.run.asking);
            dev_screenshot::answer(connection, runs, asking, &id, window).await;
        }
        Ok(Some(Request::Input { id, window, input })) => {
            let (runs, asking) = (&context.run.runs, &context.run.asking);
            dev_input::answer(connection, runs, asking, &id, window, input).await;
        }
        Ok(Some(Request::Settings { id })) => {
            dev_settings::answer(connection, &context.settings, &id, None).await;
        }
        Ok(Some(Request::Set { id, row })) => {
            dev_settings::answer(connection, &context.settings, &id, Some(row)).await;
        }
        Ok(Some(Request::Inspect { id, asked })) => {
            let inspection = InspectionContext {
                runs: &context.run.runs,
                components: &context.run.components,
                engine: &context.run.engine,
            };
            dev_inspect::answer(connection, inspection, &id, asked).await;
        }
        Ok(Some(Request::Resize {
            id,
            window,
            width,
            height,
        })) => {
            let size = Size { width, height };
            dev_resize::answer(connection, &context.run.runs, &id, window, size).await;
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "a connection ended before it asked anything"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use serde::de::DeserializeOwned;
    use waos::{RunningProgram, WaOS};
    use waos_dev_protocol::ClientStream;
    use waos_dev_protocol::{
        Answer, Asked, Component, Connection, FileDialog, Hello, Locality, OpenAnswer,
        OutputStream, Question, RefusalKind, Request, Row, Run, RunEnd, RunEvent, Upload,
        component_digest,
    };
    use waos_package::ProgramId;

    use super::*;
    use crate::logic::environments::ProgramEnvironments;
    use crate::logic::open_command::{self, Sent};
    use crate::logic::opened_package::tests::write_package;
    use crate::logic::program_places::{ProgramPlace, ProgramPlaces};
    use crate::logic::quit_question::every_program_agrees;
    use crate::logic::runs::Runs;
    use crate::logic::sent_components::HeldWrites;
    use crate::logic::test_engine::{self, NoWindows, REFUSAL};

    /// How long a test waits for a program to do something.
    pub(crate) const WITHIN: Duration = Duration::from_secs(60);

    /// A server of the test's own: its socket, its components and its code cache in a folder of
    /// its own, removed with it.
    pub(crate) struct TestServer {
        pub(crate) folder: PathBuf,
        pub(crate) socket: PathBuf,
        runs: Arc<Runs>,
        pub(crate) store: waos_store::Store,
        /// What the server's programs' users are asked, which a test notes as the prompts do.
        pub(crate) asking: Arc<AskingNow>,
        installed: InstalledRuns,
        components: Arc<SentComponents>,
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
        #[cfg(target_vendor = "apple")]
        pub(crate) fn drawing(test: &str) -> TestServer {
            TestServer::over(test, test_engine::drawing::engine)
        }

        /// A server whose programs' views draw in no window, as a phone's deck shows a tab.
        #[cfg(target_vendor = "apple")]
        pub(crate) fn drawing_in_no_window(test: &str) -> TestServer {
            TestServer::over(test, test_engine::drawing::deck_engine)
        }

        /// A server of the test's own over the engine `engine` makes, keeping its compiled
        /// programs where it is given.
        fn over(test: &str, engine: impl FnOnce(&Path) -> Arc<WaOS>) -> TestServer {
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
            let asking = AskingNow::new();
            let components = Arc::new(SentComponents::new(folder.join("dev")));
            let server = DevServer::start(
                installed.clone(),
                store.clone(),
                Arc::clone(&asking),
                &socket,
                Arc::clone(&components),
                None,
            )
            .expect("the server binds");
            TestServer {
                folder,
                socket,
                runs,
                store,
                asking,
                installed,
                components,
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

        /// A client connected and agreed, its `Run` of `component` as the program `id` sent.
        pub(crate) async fn run(&self, id: &str, component: &Path) -> Connection<ClientStream> {
            self.run_with_variables(id, component, Vec::new()).await
        }

        /// A client connected and agreed, its `Run` of `component` as the program `id` sent, with
        /// `variables` added to the program's environment.
        pub(crate) async fn run_with_variables(
            &self,
            id: &str,
            component: &Path,
            variables: Vec<(String, String)>,
        ) -> Connection<ClientStream> {
            self.run_request(id, component, Vec::new(), variables).await
        }

        /// A client connected and agreed, its `Run` of `component` as the program `id` sent with
        /// `args` as its arguments.
        pub(crate) async fn run_with_args(
            &self,
            id: &str,
            component: &Path,
            args: Vec<String>,
        ) -> Connection<ClientStream> {
            self.run_request(id, component, args, Vec::new()).await
        }

        /// A client connected and agreed, its `Run` of `component` as the program `id` sent, with
        /// `args` as its arguments and `variables` added to its environment.
        async fn run_request(
            &self,
            id: &str,
            component: &Path,
            args: Vec<String>,
            variables: Vec<(String, String)>,
        ) -> Connection<ClientStream> {
            let component = fs::read(component).unwrap();
            let mut client = self.client().await;
            client
                .send(&Request::Run(Run {
                    id: id.parse().unwrap(),
                    args,
                    variables,
                    length: component.len() as u64,
                    digest: component_digest(&component),
                }))
                .await
                .unwrap();
            if next::<Upload>(&mut client).await == Upload::Send {
                client.send_component(&component).await.unwrap();
            }
            client
        }

        /// A client connected, its hello and WaOS's exchanged.
        async fn client(&self) -> Connection<ClientStream> {
            let mut client = Connection::connect(&self.socket).await.unwrap();
            client
                .hello(&Hello::cargo_waos("cargo-waos test"))
                .await
                .unwrap();
            client
        }

        /// A client connected and agreed, its `Run` of `component`'s bytes as the program `id`
        /// sent, the component in parts of at most `parts` bytes, announcing `digest` for it.
        pub(crate) async fn run_in_parts_of(
            &self,
            id: &str,
            component: &[u8],
            parts: usize,
            digest: [u8; 32],
        ) -> Connection<ClientStream> {
            let mut client = self.client().await;
            client
                .send(&Request::Run(Run {
                    id: id.parse().unwrap(),
                    args: Vec::new(),
                    variables: Vec::new(),
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
            client
        }

        /// What WaOS answers `request`, sent on a connection of its own.
        pub(crate) async fn ask<A: DeserializeOwned>(&self, request: &Request) -> A {
            let mut client = self.client().await;
            client.send(request).await.unwrap();
            next(&mut client).await
        }

        /// A client connected over a network transport and agreed.
        #[cfg(unix)]
        async fn network_client(&self) -> Connection<ClientStream> {
            let (client, server) = tokio::net::UnixStream::pair().unwrap();
            let context = ConnectionContext {
                run: RunContext {
                    engine: Arc::clone(&self.installed.engine),
                    environments: Arc::clone(&self.installed.environments),
                    runs: Arc::clone(&self.installed.runs),
                    components: Arc::clone(&self.components),
                    dev_compiles: None,
                    asking: Arc::clone(&self.asking),
                },
                installed: self.installed.clone(),
                store: self.store.clone(),
                settings: SettingsContext {
                    runs: Arc::clone(&self.installed.runs),
                    components: Arc::clone(&self.components),
                    store: self.store.clone(),
                    environments: Arc::clone(&self.installed.environments),
                },
            };
            tokio::spawn(serve_connection(
                Connection::over(server, Locality::Network),
                context,
            ));
            let mut client = Connection::over(client, Locality::Network);
            let ours = Hello::cargo_waos("cargo-waos 0.3.0");
            let theirs = client.hello(&ours).await.unwrap();
            assert_eq!(theirs.major, waos_dev_protocol::MAJOR);
            client
        }

        /// Holds back the write of every component sent from now on, and so its load, until the
        /// answer is dropped.
        fn hold_loads(&self) -> HeldWrites {
            let server = self.server.as_ref().expect("the server runs");
            server.components.hold_writes()
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
    pub(crate) async fn next<A: DeserializeOwned>(client: &mut Connection<ClientStream>) -> A {
        tokio::time::timeout(WITHIN, client.receive::<A>())
            .await
            .expect("a frame in time")
            .expect("a frame, not an error")
            .expect("a frame, not the close")
    }

    /// The first frames of a run that starts: its component is compiled, then it runs.
    pub(crate) async fn started(client: &mut Connection<ClientStream>) {
        assert_eq!(next::<RunEvent>(client).await, RunEvent::Compiling);
        assert_eq!(next::<RunEvent>(client).await, RunEvent::Started);
    }

    /// The first frame of a run whose component's code is compiled already: it runs.
    async fn started_from_compiled_code(client: &mut Connection<ClientStream>) {
        assert_eq!(next::<RunEvent>(client).await, RunEvent::Started);
    }

    /// Every frame up to and including the first `wanted` accepts.
    async fn frames_until(
        client: &mut Connection<ClientStream>,
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

    /// The id arguments runs as.
    const ARGUMENTS: &str = "dev.waos.examples.arguments";

    #[tokio::test]
    async fn a_runs_output_reaches_the_client_in_order_after_started() {
        let server = TestServer::start("output");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        let mut frames = Vec::new();
        while !stdout_of(&frames).contains(FIRST_REPORTS) {
            frames.push(next(&mut client).await);
        }
        assert_eq!(frames[..2], [RunEvent::Compiling, RunEvent::Started]);
        assert!(
            frames[2..]
                .iter()
                .all(|frame| matches!(frame, RunEvent::Output { .. })),
            "{frames:?}"
        );
        assert!(stdout_of(&frames).starts_with(FIRST_REPORTS));
    }

    #[tokio::test]
    async fn closing_the_connection_stops_the_program() {
        let server = TestServer::start("closing");
        let client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        server.wait_for_runs(&[FOREVER]).await;
        drop(client);
        server.wait_for_runs(&[]).await;
    }

    #[tokio::test]
    async fn a_second_run_of_the_same_name_ends_the_first_with_replaced() {
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
        started_from_compiled_code(&mut second).await;
        server.wait_for_runs(&[FOREVER]).await;
    }

    #[tokio::test]
    async fn a_component_sent_again_unchanged_runs_from_the_code_compiled_before() {
        let server = TestServer::start("unchanged");
        let component = Path::new(waos_test_programs::FOREVER);
        let mut first = server.run(FOREVER, component).await;
        started(&mut first).await;
        drop(first);
        server.wait_for_runs(&[]).await;
        let mut again = server.run(FOREVER, component).await;
        started_from_compiled_code(&mut again).await;
    }

    #[tokio::test]
    async fn a_component_waos_holds_is_answered_have_and_nothing_is_sent() {
        let server = TestServer::start("have");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        let mut first = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut first).await;
        drop(first);
        server.wait_for_runs(&[]).await;
        let mut again = server.client().await;
        again
            .send(&Request::Run(Run {
                id: FOREVER.parse().unwrap(),
                args: Vec::new(),
                variables: Vec::new(),
                length: component.len() as u64,
                digest: component_digest(&component),
            }))
            .await
            .unwrap();
        assert_eq!(next::<Upload>(&mut again).await, Upload::Have);
        // Nothing is sent for a component WaOS holds: the file is what this build compiled.
        started_from_compiled_code(&mut again).await;
    }

    #[tokio::test]
    async fn a_component_sent_changed_is_compiled_again() {
        let server = TestServer::start("changed");
        let mut first = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut first).await;
        drop(first);
        server.wait_for_runs(&[]).await;
        let mut changed = server
            .run(FOREVER, Path::new(waos_test_programs::OPENER))
            .await;
        started(&mut changed).await;
    }

    #[tokio::test]
    async fn a_run_being_loaded_holds_back_no_other_programs_tools() {
        let server = TestServer::start("loading");
        let mut running = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut running).await;
        let held = server.hold_loads();
        let mut loading = server
            .run(OPENER, Path::new(waos_test_programs::OPENER))
            .await;
        held.a_write_waits().await;
        let screenshot = Request::Screenshot {
            id: FOREVER.parse().unwrap(),
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
        started(&mut loading).await;
    }

    #[tokio::test]
    async fn a_runs_arguments_reach_the_program() {
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
        assert_eq!(stdout_of(&frames), "argument: one\nargument: two words\n");
    }

    #[tokio::test]
    async fn a_run_is_the_program_its_id_names_with_its_own_folders() {
        let server = TestServer::start("folders");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        let program = server.folder.join("store/programs").join(FOREVER);
        assert!(program.join("data").is_dir(), "made at its first run");
        assert!(program.join("cache").is_dir());
        let names = server.runs.programs().await;
        assert_eq!(names[0].name(), FOREVER);
    }

    #[tokio::test]
    async fn a_programs_settings_are_read_before_its_first_run_and_set_at_any_time() {
        let server = TestServer::start("settings");
        let id: ProgramId = FOREVER.parse().unwrap();
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
            !server.folder.join("store/programs").join(FOREVER).exists(),
            "reading the rows keeps nothing"
        );
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        drop(client);
        server.wait_for_runs(&[]).await;

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
    async fn the_camera_and_the_microphone_are_set_simulated_the_microphone_with_what_it_records() {
        let server = TestServer::start("set-simulated");
        let mut client = server
            .run(FOREVER, Path::new(waos_test_programs::FOREVER))
            .await;
        started(&mut client).await;
        let id: ProgramId = FOREVER.parse().unwrap();
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
        let id: ProgramId = FOREVER.parse().unwrap();
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
            id: FOREVER.parse().unwrap(),
            window: waos_dev_protocol::WindowId(1),
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
    async fn a_program_that_returns_ends_returned_with_what_it_wrote_before() {
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
    }

    #[tokio::test]
    async fn the_variables_a_run_adds_reach_the_program_and_waos_s_own_win() {
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
            "the run's variable, and WaOS's own TMPDIR over one of that name"
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
    async fn a_component_that_is_not_one_is_refused() {
        let server = TestServer::start("refused");
        let mut client = server
            .run_in_parts_of(
                "dev.waos.examples.nonsense",
                b"not a component",
                4,
                component_digest(b"not a component"),
            )
            .await;
        assert_eq!(next::<RunEvent>(&mut client).await, RunEvent::Compiling);
        let RunEvent::Refused(refusal) = next::<RunEvent>(&mut client).await else {
            panic!("refused");
        };
        assert!(refusal.message.contains("nonsense"), "{refusal:?}");
        assert_eq!(client.receive::<RunEvent>().await.unwrap(), None, "closed");
        server.wait_for_runs(&[]).await;
    }

    #[tokio::test]
    async fn a_component_sent_in_parts_is_assembled_and_runs() {
        let server = TestServer::start("parts");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        // Parts far under a frame's worth, so the component spans many of them.
        let mut client = server
            .run_in_parts_of(FOREVER, &component, 100_000, component_digest(&component))
            .await;
        started(&mut client).await;
        server.wait_for_runs(&[FOREVER]).await;
    }

    #[tokio::test]
    async fn a_component_whose_digest_is_not_the_one_announced_is_refused() {
        let server = TestServer::start("wrong-digest");
        let component = fs::read(waos_test_programs::FOREVER).unwrap();
        let mut client = server
            .run_in_parts_of(FOREVER, &component, 100_000, [0xab; 32])
            .await;
        let RunEvent::Refused(refusal) = next::<RunEvent>(&mut client).await else {
            panic!("refused");
        };
        assert!(refusal.message.contains("digest"), "{refusal:?}");
        assert_eq!(client.receive::<RunEvent>().await.unwrap(), None, "closed");
        server.wait_for_runs(&[]).await;
    }

    #[tokio::test]
    async fn a_client_of_another_protocol_major_is_refused() {
        let server = TestServer::start("protocol");
        let mut client = Connection::connect(&server.socket).await.unwrap();
        let mut newer = Hello::cargo_waos("cargo-waos 0.4.0");
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
        let id = OPENER.parse().unwrap();
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

    #[cfg(unix)]
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
}
