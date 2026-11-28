//! The app instances in a test's WaOS: an update of a running app stops its instance first, and
//! waits for it.

use waos::ProgramEnd;
use waos_dev_protocol::{RunEnd, RunEvent};
use waos_package::Manifest;

use super::*;
use crate::logic::startup::Setup;
use crate::logic::startup::tests::start_asking_nobody;
use crate::logic::test_engine::{self, NoWindows};
use crate::logic::test_installs;

/// Forever's manifest at `version`, which runs until it is stopped.
fn forever_at(version: &str) -> Manifest {
    Manifest::new(
        "dev.example.forever".parse().unwrap(),
        "Forever",
        version.parse().unwrap(),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_update_of_a_running_app_stops_its_instance_first_and_waits_for_it() {
    let folder = tempfile::tempdir().unwrap();
    let setup = Setup::on_files(folder.path(), tokio::runtime::Handle::current());
    let started = start_asking_nobody(setup, |_before| {
        (
            test_engine::shell(Arc::new(NoWindows)),
            test_engine::no_screens(),
        )
    });
    let (instances, packages) = (started.instances, started.packages);
    let forever = waos_test_programs::FOREVER;
    let installed = test_installs::install_development(&packages, forever_at("0.1.0"), forever);
    let id = installed.manifest.id().clone();
    let configuration = OpenConfiguration::new(OpenedFor::Development);
    let opened = instances.open(&id, configuration).await.expect("it opens");
    let before = opened.instance().clone();
    let output = instances.outputs().of(&id).expect("its output").attach();

    let update = tokio::task::spawn_blocking(move || {
        test_installs::install_development(&packages, forever_at("0.2.0"), forever)
    });
    update.await.expect("the update is installed");
    assert!(
        matches!(before.ended().now_or_never(), Some(ProgramEnd::Stopped)),
        "the instance had ended by the time the update was in place"
    );
    assert!(
        instances.instance(&id).is_none(),
        "and it is out of the list"
    );
    let mut last = None;
    while let Some(event) = output.next().await {
        last = Some(event);
    }
    assert_eq!(
        last,
        Some(RunEvent::Ended(RunEnd::Replaced)),
        "it ends replaced"
    );
}
