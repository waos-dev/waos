//! `examples/waft-time`, a Waft app over `wasdk-adapter-waft`, under the test shell: its local
//! `DateTime`'s hour, zone name and offset from UTC are the user's time zone's, read through the
//! adapter's `time_zone_offset` and `time_zone_name`, and its UTC value is the same instant.
//! It is a debug build of Waft, slow to start, and draws, so it runs where the engine makes
//! hardware buffers, on Apple's systems.

use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, ProgramEnvironment, ProgramOutput, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, within};

#[test]
fn a_local_date_time_reads_the_users_hour_and_zone_name() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    engine.set_time_zone(Some("America/New_York".into()));
    let program = engine
        .load(Path::new(waos_test_programs::WAFT_TIME))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&output) as Arc<dyn ProgramOutput>),
        ..waos_local::LocalCapabilities::new().environment()
    };
    let running = engine.run(&program, environment).unwrap();
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("waft-time asks for its window");

    output.wait_for("local: 0 EST");
    output.wait_for("utc: 5, offset -5 h");

    view.close_requested();
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}
