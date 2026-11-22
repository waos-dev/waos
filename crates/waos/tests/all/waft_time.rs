//! `examples/waft-time`, a Waft app over `wasdk-adapter-waft`, under the test shell: its local
//! `DateTime`'s hour, zone name and offset from UTC are the user's time zone's, read through the
//! adapter's `time_zone_offset` and `time_zone_name`, and its UTC value is the same instant.
//! It is a debug build of Waft, slow to start, and draws, so it runs where the engine makes
//! hardware buffers, on Apple's systems.

use std::sync::Arc;

use waos::{ProgramEnd, ProgramEnvironment, ProgramOutput};

use crate::shell::{self, Lines, WITHIN};

#[test]
fn a_local_date_time_reads_the_users_hour_and_zone_name() {
    let (engine, _shell, arrivals) = shell::engine();
    engine.set_time_zone(Some("America/New_York".into()));
    let lines = Arc::new(Lines::default());
    let program = shell::load(&engine, waos_test_programs::WAFT_TIME);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        ..waos_local::LocalCapabilities::new().environment()
    };
    let running = engine.run(&program, environment).unwrap();
    let view = arrivals
        .recv_timeout(WITHIN)
        .expect("waft-time asks for its window");

    lines.wait_for("local: 0 EST");
    lines.wait_for("utc: 5, offset -5 h");

    view.close_requested();
    let end = shell::wait_for_end(&running);
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}
