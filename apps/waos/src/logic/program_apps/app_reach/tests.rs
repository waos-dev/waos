use std::path::Path;
use std::time::Duration;

use futures::channel::oneshot;
use futures::{FutureExt, StreamExt};
use waos::{ProgramEnd, ProgramEnvironment};

use super::*;
use crate::logic::program_apps::CanvasCall;
use crate::logic::program_apps::window_process::tests::{Served, window_process};
use crate::logic::test_engine;
use crate::logic::test_engine::WITHIN;

/// The id survey runs under, as an installed program's.
const SURVEY: &str = "dev.example.survey";

/// The launches the getter asked for, in order.
#[derive(Clone, Default)]
pub(crate) struct Launches(Arc<Mutex<Vec<(ProgramId, LaunchAttempt)>>>);

impl Launches {
    /// The getter, whose launches start nothing and are noted here.
    pub(crate) fn reach(&self) -> Arc<AppReach> {
        let noted = self.clone();
        Arc::new(AppReach::new(move |id, attempt| {
            noted.0.lock().unwrap().push((id.clone(), attempt));
        }))
    }

    pub(crate) fn asked(&self) -> Vec<(ProgramId, LaunchAttempt)> {
        self.0.lock().unwrap().clone()
    }

    /// Waits until `count` launches were asked for.
    pub(crate) async fn until(&self, count: usize) {
        tokio::time::timeout(WITHIN, async {
            while self.asked().len() < count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{count} launches were never asked for"));
    }
}

/// Tells `reach` that `process`, `id`'s app, said hello, as `ProgramApps` tells it, for a test
/// outside the programs' apps.
pub(crate) fn said_hello(reach: &AppReach, id: &ProgramId, process: &Arc<WindowProcess>) {
    reach.connected(id, process);
}

/// Tells `reach` that the launch of `id`'s app failed, for `reason`, as `ProgramApps` tells it, for
/// a test outside the programs' apps.
pub(crate) fn launch_failed(reach: &AppReach, id: &ProgramId, reason: &str) {
    reach.launch_failed(id, reason);
}

/// Survey, under its id, its windows placed by `reach`, as the engine places a program's on macOS.
fn survey_through(reach: &Arc<AppReach>) -> RunningProgram {
    let placement = Arc::clone(reach) as Arc<dyn ViewPlacement>;
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

/// Asks `reach` for `program`'s app now; resolves with what it hands over, once it does.
fn asked(reach: &AppReach, program: &RunningProgram) -> impl Future<Output = ReachedApp> + use<> {
    let (answer, answered) = oneshot::channel();
    reach.with_app(program, move |app| {
        let _ = answer.send(app);
    });
    answered.map(|app| app.expect("the getter answers"))
}

#[tokio::test]
async fn everything_that_asks_while_an_app_launches_shares_one_launch_and_gets_the_app() {
    let launches = Launches::default();
    let reach = launches.reach();
    let program = survey_through(&reach);
    launches.until(1).await;
    assert!(
        reach.running_app(&program).is_none(),
        "the app does not run yet"
    );
    let first = asked(&reach, &program);
    let second = asked(&reach, &program);
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
    reach.connected(&survey_id(), &process);
    let made = tokio::time::timeout(WITHIN, canvas_calls.next()).await;
    assert!(
        matches!(made, Ok(Some(CanvasCall::Make { .. }))),
        "the window that waited opens in the app"
    );
    for reached in [first.await, second.await] {
        let reached = reached.expect("the app came");
        assert!(Arc::ptr_eq(&reached, &process), "the one app");
    }
    let running = reach.running_app(&program).expect("the app runs");
    assert!(Arc::ptr_eq(&running, &process));
    assert!(Arc::ptr_eq(
        &asked(&reach, &program).await.unwrap(),
        &process
    ));
    assert_eq!(launches.asked().len(), 1, "a running app is not launched");
}

#[tokio::test]
async fn an_app_that_never_comes_is_launched_once_more_then_fails_everything_that_waited() {
    let launches = Launches::default();
    let reach = launches.reach();
    let program = survey_through(&reach);
    launches.until(1).await;
    let waiting = asked(&reach, &program);

    reach.started(&survey_id(), 4242);
    reach.ended(&survey_id(), 4242);
    assert_eq!(
        launches.asked(),
        [
            (survey_id(), LaunchAttempt::First),
            (survey_id(), LaunchAttempt::Again)
        ],
        "an app that ended before its hello is launched once more"
    );
    reach.launch_failed(&survey_id(), "Launch Services: no app");

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
