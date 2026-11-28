//! The app instances in a test's WaOS kept in memory, its apps prefilled from the test programs'
//! builds: an app opened again runs once; a launch replaces the instance running and leaves the
//! app's place as it was; a tool opened from an icon is refused, one opened for files runs; an
//! update or a removal of a running app stops its instance first; and an open whose code this
//! engine has not compiled is told optimizing while it compiles.

use std::path::Path;

use waos::ProgramEnd;
use waos_dev_protocol::{RunEnd, RunEvent};
use waos_package::Manifest;
use waos_services::packages::{AppInMemory, CodeCompiler};

use super::*;
use crate::logic::launch_outputs::Frames;
use crate::logic::startup::tests::start_with_no_windows;
use crate::logic::startup::{Setup, Started};
use crate::logic::test_installs::{self, HeldCompiler};

const FOREVER: &str = "dev.example.forever";

fn forever_id() -> ProgramId {
    FOREVER.parse().unwrap()
}

/// Forever, built, which runs until it is stopped.
fn forever() -> AppInMemory {
    test_installs::built_app(waos_test_programs::FOREVER, FOREVER, "Forever")
}

/// A test's WaOS kept in memory, with `apps` installed, their own folders in `folder`.
fn started_with(folder: &Path, apps: Vec<AppInMemory>) -> Started {
    let runtime = tokio::runtime::Handle::current();
    start_with_no_windows(Setup::in_memory(folder, apps, runtime))
}

/// What `output` heard last, once its run ended.
async fn last_heard(output: Arc<Frames>) -> Option<RunEvent> {
    let mut last = None;
    while let Some(event) = output.next().await {
        last = Some(event);
    }
    last
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_opened_again_runs_once() {
    let folder = tempfile::tempdir().unwrap();
    let started = started_with(folder.path(), vec![forever()]);
    let user = || OpenConfiguration::new(OpenedFor::User);

    let first = started.instances.open(&forever_id(), user()).await;
    let again = started.instances.open(&forever_id(), user()).await;

    let (first, again) = (first.expect("it opens"), again.expect("it opens again"));
    assert!(matches!(again, Opened::RunningAlready(_)), "nothing starts");
    assert!(
        again.instance() == first.instance(),
        "the running one found"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_launch_replaces_the_instance_running_and_leaves_the_apps_place_as_it_was() {
    let folder = tempfile::tempdir().unwrap();
    let started = started_with(folder.path(), vec![forever()]);
    let (instances, id) = (&started.instances, forever_id());
    let user = OpenConfiguration::new(OpenedFor::User);
    instances.open(&id, user).await.expect("it opens");
    let output = instances.outputs().of(&id).expect("its output").attach();
    started.places.set(&id, ProgramPlace::Tab);

    let launch = OpenConfiguration {
        replaces_running: true,
        place: OpenPlace::AsItWas,
        ..OpenConfiguration::new(OpenedFor::Development)
    };
    let launched = instances.open(&id, launch).await.expect("it launches");

    assert!(matches!(launched, Opened::Started { .. }), "started anew");
    assert_eq!(
        last_heard(output).await,
        Some(RunEvent::Ended(RunEnd::Replaced)),
        "the one before ends replaced"
    );
    assert_eq!(started.places.place_of(&id), ProgramPlace::Tab);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_opened_from_an_icon_is_refused_and_one_opened_for_files_runs() {
    let folder = tempfile::tempdir().unwrap();
    let mut tool = forever();
    tool.program.manifest = tool.program.manifest.with_kind(ProgramKind::Tool);
    let started = started_with(folder.path(), vec![tool]);
    let (instances, id) = (&started.instances, forever_id());

    let from_an_icon = instances.open(&id, OpenConfiguration::new(OpenedFor::User));
    assert!(matches!(from_an_icon.await, Err(OpenError::Tool(_))));
    assert!(instances.instance(&id).is_none(), "it never runs");

    let for_files = instances.open(&id, OpenConfiguration::new(OpenedFor::HandOver));
    for_files.await.expect("it opens");
    assert!(instances.instance(&id).is_some(), "it runs");
}

/// Forever's manifest at `version`, as its development builds are.
fn forever_at(version: &str) -> Manifest {
    Manifest::new(forever_id(), "Forever", version.parse().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_update_of_a_running_app_stops_its_instance_first_and_waits_for_it() {
    let folder = tempfile::tempdir().unwrap();
    let id = forever_id().development();
    let built = test_installs::built_app(waos_test_programs::FOREVER, id.as_str(), "Forever");
    let started = started_with(folder.path(), vec![built]);
    let (instances, packages) = (started.instances, started.packages);
    let configuration = OpenConfiguration::new(OpenedFor::Development);
    let opened = instances.open(&id, configuration).await.expect("it opens");
    let before = opened.instance().clone();
    let output = instances.outputs().of(&id).expect("its output").attach();

    let update = tokio::task::spawn_blocking(move || {
        let forever = waos_test_programs::FOREVER;
        test_installs::install_development(&packages, forever_at("0.2.0"), forever)
    });
    update.await.expect("the update is installed");

    assert!(
        matches!(before.ended().now_or_never(), Some(ProgramEnd::Stopped)),
        "the instance had ended by the time the update was in place"
    );
    assert!(instances.instance(&id).is_none(), "and is out of the list");
    assert_eq!(
        last_heard(output).await,
        Some(RunEvent::Ended(RunEnd::Replaced)),
        "it ends replaced"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removal_of_a_running_app_stops_its_instance_first_and_waits_for_it() {
    let folder = tempfile::tempdir().unwrap();
    let started = started_with(folder.path(), vec![forever()]);
    let (instances, id) = (&started.instances, forever_id());
    let configuration = OpenConfiguration::new(OpenedFor::User);
    let opened = instances.open(&id, configuration).await.expect("it opens");
    let before = opened.instance().clone();
    let output = instances.outputs().of(&id).expect("its output").attach();

    started.packages.remove(&id).await.expect("it is removed");

    assert!(
        matches!(before.ended().now_or_never(), Some(ProgramEnd::Stopped)),
        "the instance had ended by the time it was removed"
    );
    assert!(instances.instance(&id).is_none(), "and is out of the list");
    assert_eq!(
        last_heard(output).await,
        Some(RunEvent::Ended(RunEnd::Stopped)),
        "it ends stopped"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_open_is_told_optimizing_while_its_code_compiles() {
    let folder = tempfile::tempdir().unwrap();
    let app = test_installs::built_app_not_compiled(
        waos_test_programs::FOREVER,
        FOREVER,
        "Forever",
        folder.path(),
    );
    let compiler = Arc::new(HeldCompiler::default());
    let setup = Setup {
        install_compiler: Arc::clone(&compiler) as Arc<dyn CodeCompiler>,
        ..Setup::in_memory(folder.path(), vec![app], tokio::runtime::Handle::current())
    };
    let started = start_with_no_windows(setup);
    let (instances, id) = (Arc::clone(&started.instances), forever_id());
    let mut opening = instances.opening();
    let held = compiler.hold();

    let open = tokio::spawn({
        let (instances, id) = (Arc::clone(&instances), id.clone());
        async move {
            let configuration = OpenConfiguration::new(OpenedFor::User);
            instances.open(&id, configuration).await.map(|_| ())
        }
    });
    opening
        .wait_for(|stages| stages.get(&id) == Some(&OpenStage::Optimizing))
        .await
        .expect("told optimizing while it compiles");
    drop(held);

    open.await.unwrap().expect("it opens once compiled");
    assert!(instances.instance(&id).is_some(), "it runs");
    assert!(instances.opening().borrow().is_empty(), "no open under way");
}
