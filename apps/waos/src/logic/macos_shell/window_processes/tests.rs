use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::channel::mpsc::{self, UnboundedReceiver};
use futures::{FutureExt, StreamExt};
use waos::{OpenRequest, ProgramEnd, ProgramEnvironment};
use waos_headless::testing::Changes;
use waos_window_messages::os::macos::{Arrival, ReceiveRight, SendRight};
use waos_window_messages::{FromWindow, Hello, LaunchReason, PROTOCOL, ToWindow, decode, encode};

use super::*;
use crate::logic::macos_shell::CanvasCall;
use crate::logic::macos_shell::window_process::tests::{Served, window_process};
use crate::logic::test_engine;
use crate::logic::test_engine::WITHIN;

/// The id survey runs under, as an installed program's.
const SURVEY: &str = "dev.example.survey";

/// The apps' bundles as a test stands them in: each launch asked for is noted, in order, and
/// answered as the test says; every window process that says hello is the one WaOS signed.
#[derive(Clone, Default)]
pub(crate) struct Launches(Arc<AskedLaunches>);

#[derive(Default)]
pub(crate) struct AskedLaunches {
    asked: Mutex<Vec<AskedLaunch>>,
    changes: Changes,
}

/// One launch asked for, and where its answer goes until the test gives it.
struct AskedLaunch {
    app: ProgramId,
    attempt: LaunchAttempt,
    answer: Option<oneshot::Sender<Result<LaunchedProcess, LaunchFailure>>>,
}

/// Window processes over stand-in bundles, listening under a name of the test's own, and the apps
/// the user opens through them.
pub(crate) struct Listening {
    pub(crate) window_processes: Arc<WindowProcesses>,
    pub(crate) name: String,
    pub(crate) user_opens: UnboundedReceiver<UserOpen>,
}

impl Launches {
    /// The window processes, launched from these bundles.
    pub(crate) fn window_processes(&self) -> Arc<WindowProcesses> {
        self.listening().window_processes
    }

    /// The window processes, launched from these bundles, and where they listen.
    pub(crate) fn listening(&self) -> Listening {
        static LISTENERS: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "dev.waos.tests.window-processes.{}.{}",
            std::process::id(),
            LISTENERS.fetch_add(1, Ordering::Relaxed)
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("a runtime");
        let (canvases, _canvas_calls) = CanvasesOnRequest::new();
        let (dialog_reports, _reported) = mpsc::unbounded();
        let (notification_permissions, _readings) = NotificationPermissions::new();
        let (window_presences, _presences) = mpsc::unbounded();
        let routes = Routes {
            canvases: Arc::new(canvases),
            dialog_reports,
            notification_permissions,
            window_presences,
        };
        let (opens, user_opens) = mpsc::unbounded();
        let bundles = Arc::clone(&self.0) as Arc<dyn Bundles>;
        let window_processes = WindowProcesses::start(runtime, bundles, routes, &name, opens)
            .expect("the window processes listen");
        Listening {
            window_processes,
            name,
            user_opens,
        }
    }

    /// Each launch asked for, in order.
    pub(crate) fn asked(&self) -> Vec<(ProgramId, LaunchAttempt)> {
        let asked = self.0.asked.lock().unwrap();
        (asked.iter())
            .map(|launch| (launch.app.clone(), launch.attempt))
            .collect()
    }

    /// Waits until `count` launches were asked for.
    pub(crate) async fn until(&self, count: usize) {
        self.0
            .changes
            .until(&format!("{count} launches asked for"), || {
                self.asked().len() >= count
            })
            .await;
    }

    /// Answers the launch `index`, once asked for, as Launch Services does that starts no app,
    /// for `reason`.
    pub(crate) async fn fail(&self, index: usize, reason: &str) {
        let failure = LaunchFailure::LaunchServices(reason.to_owned());
        self.answer(index, Err(failure)).await;
    }

    /// Answers the launch `index`, once asked for, with `answer`.
    async fn answer(&self, index: usize, answer: Result<LaunchedProcess, LaunchFailure>) {
        self.until(index + 1).await;
        let answering = self.0.asked.lock().unwrap()[index].answer.take();
        let _ = answering.expect("a launch answered once").send(answer);
    }
}

impl Bundles for AskedLaunches {
    fn launch(
        self: Arc<Self>,
        app: ProgramId,
        rewrite: bool,
    ) -> BoxFuture<'static, Result<LaunchedProcess, LaunchFailure>> {
        let (answer, answered) = oneshot::channel();
        let attempt = if rewrite {
            LaunchAttempt::Again
        } else {
            LaunchAttempt::First
        };
        self.asked.lock().unwrap().push(AskedLaunch {
            app,
            attempt,
            answer: Some(answer),
        });
        self.changes.announce();
        let let_go = || LaunchFailure::Bundle("the test let go of the launch".to_owned());
        answered
            .map(move |answered| answered.unwrap_or_else(|_| Err(let_go())))
            .boxed()
    }

    fn check(self: Arc<Self>, _app: &ProgramId, _sender: &AuditToken) -> Result<(), String> {
        Ok(())
    }

    fn name(&self, program: &RunningProgram) -> String {
        program.name().to_owned()
    }
}

/// Tells `window_processes` that `process`, `id`'s window process, said hello, as the listener
/// tells them, for a test outside the connection.
pub(crate) fn said_hello(
    window_processes: &WindowProcesses,
    id: &ProgramId,
    process: &Arc<WindowProcess>,
) {
    window_processes.table().connected(id, process);
}

/// Survey, under its id, its windows placed by `window_processes`, as the engine places an app's
/// on macOS.
fn survey_through(window_processes: &Arc<WindowProcesses>) -> RunningProgram {
    let placement = Arc::clone(window_processes) as Arc<dyn ViewPlacement>;
    let engine = test_engine::engine(placement);
    let survey = engine
        .load(
            Path::new(waos_test_programs::SURVEY),
            &waos_test_programs::code_of(waos_test_programs::SURVEY),
        )
        .expect("survey loads")
        .named(SURVEY);

    engine
        .run(&survey, ProgramEnvironment::default())
        .expect("survey runs")
}

fn survey_id() -> ProgramId {
    SURVEY.parse().unwrap()
}

/// Asks `window_processes` for `program`'s window process now; resolves with what they hand
/// over, once they do.
fn asked(
    window_processes: &WindowProcesses,
    program: &RunningProgram,
) -> impl Future<Output = ReachedApp> + use<> {
    window_processes.of(program)
}

#[tokio::test]
async fn everything_that_asks_while_an_app_launches_shares_one_launch_and_gets_the_app() {
    let launches = Launches::default();
    let window_processes = launches.window_processes();
    let program = survey_through(&window_processes);
    launches.until(1).await;
    assert!(
        window_processes.running(&program).is_none(),
        "the app does not run yet"
    );
    let first = asked(&window_processes, &program);
    let second = asked(&window_processes, &program);
    assert_eq!(
        launches.asked(),
        [(survey_id(), LaunchAttempt::First)],
        "survey's window launched the app, and what asked after waits for that launch"
    );

    let Served {
        process,
        mut canvas_calls,
        ..
    } = window_process(program.clone());
    said_hello(&window_processes, &survey_id(), &process);
    let made = tokio::time::timeout(WITHIN, canvas_calls.next()).await;
    assert!(
        matches!(made, Ok(Some(CanvasCall::Make { .. }))),
        "the window that waited opens in the app"
    );
    for reached in [first.await, second.await] {
        let reached = reached.expect("the app came");
        assert!(Arc::ptr_eq(&reached, &process), "the one app");
    }
    let running = window_processes.running(&program).expect("the app runs");
    assert!(Arc::ptr_eq(&running, &process));
    assert!(Arc::ptr_eq(
        &asked(&window_processes, &program).await.unwrap(),
        &process
    ));
    assert_eq!(launches.asked().len(), 1, "a running app is not launched");
}

#[tokio::test]
async fn an_app_that_never_comes_is_launched_once_more_then_fails_everything_that_waited() {
    let launches = Launches::default();
    let window_processes = launches.window_processes();
    let program = survey_through(&window_processes);
    launches.until(1).await;
    let waiting = asked(&window_processes, &program);

    let (exited, exit) = oneshot::channel();
    let started = LaunchedProcess {
        pid: 4242,
        exited: exit,
    };
    launches.answer(0, Ok(started)).await;
    let _ = exited.send(());
    launches.until(2).await;
    assert_eq!(
        launches.asked(),
        [
            (survey_id(), LaunchAttempt::First),
            (survey_id(), LaunchAttempt::Again)
        ],
        "an app that ended before its hello is launched once more"
    );
    launches.fail(1, "Launch Services: no app").await;

    let failed = waiting.await.err().expect("the app never came");
    assert_eq!(
        failed,
        "the program's app did not start: Launch Services: no app"
    );
    let end = tokio::time::timeout(WITHIN, program.ended())
        .await
        .expect("survey ends once its window fails");
    assert!(matches!(end, ProgramEnd::Returned(Err(()))), "{end:?}");
    assert_eq!(launches.asked().len(), 2, "no third launch");
}

#[tokio::test]
async fn the_window_processes_listen_as_long_as_the_shell_that_holds_them() {
    let Listening {
        window_processes,
        name,
        ..
    } = Launches::default().listening();
    let shell = test_engine::shell(window_processes);
    let to_waos = SendRight::look_up(&name).expect("WaOS listens while its shell is held");

    drop(shell);
    let sent = to_waos.send(PROTOCOL, b"a hello", Vec::new());
    assert!(sent.is_err(), "nothing listens once the shell is let go");
}

#[tokio::test]
async fn a_hello_the_user_opened_waits_until_the_users_opens_are_read_and_is_served_as_the_run_opened()
 {
    let launches = Launches::default();
    let Listening {
        window_processes,
        name,
        mut user_opens,
    } = launches.listening();
    let program = survey_through(&window_processes);
    let window_process = ReceiveRight::new().unwrap();
    let hello = FromWindow::Hello(Hello {
        program: survey_id(),
        launched_for: LaunchReason::UserOpened,
        opened: OpenRequest::default(),
    });
    let to_waos = SendRight::look_up(&name).unwrap();
    to_waos
        .send(
            PROTOCOL,
            &encode(&hello),
            vec![window_process.make_send().unwrap()],
        )
        .unwrap();

    let open = tokio::time::timeout(WITHIN, user_opens.next())
        .await
        .expect("the user's open is heard")
        .expect("one open");
    assert_eq!(open.app, survey_id(), "the app the user opened");
    let _ = open.opened.send(Ok(program.clone()));
    let welcomed = tokio::task::spawn_blocking(move || window_process.receive(PROTOCOL));
    let welcomed = tokio::time::timeout(WITHIN, welcomed)
        .await
        .expect("the window process is answered")
        .unwrap()
        .unwrap();
    let Arrival::Message(welcome) = welcomed else {
        panic!("an answer in this WaOS's protocol");
    };
    let Ok(ToWindow::Welcome(welcome)) = decode::<ToWindow>(&welcome.bytes) else {
        panic!("welcomed");
    };
    assert_eq!(welcome.name, SURVEY, "welcomed as the run opened");
    let served = tokio::time::timeout(WITHIN, asked(&window_processes, &program))
        .await
        .expect("the run's window process is there");
    assert_eq!(served.err(), None, "the run opened is served by it");
}
