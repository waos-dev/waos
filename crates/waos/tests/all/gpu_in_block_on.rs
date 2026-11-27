//! `examples/gpu-in-block-on` under the test shell: a program that opens a device inside
//! `wasdk::block_on`, as a winit app does inside its event loop, and submits once, gets the wait
//! back. Opening a device started tasks before, which the wait waited on forever.

use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[test]
fn a_device_opened_inside_block_on_lets_it_return() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::GPU_IN_BLOCK_ON))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&program, printing_to(&output)).unwrap();
    let end = within(running.ended());
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "{end:?}: {:?}",
        output.lines()
    );
    assert_eq!(output.lines(), ["returned"]);
}
