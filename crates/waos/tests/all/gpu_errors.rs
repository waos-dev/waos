//! `examples/gpu-errors` under the test shell: wgpu's errors reach a program as wgpu's own backends
//! route them. A scope pops at the call, so an error made before its future is awaited goes to
//! the scope outside it, and a scope whose guard is dropped unawaited is gone, so the next error
//! reaches the uncaptured-error handler. A device's loss reaches the device-lost callback set
//! last, as wgpu replaces it, and a device dropped unlost drops its callback uncalled. And
//! `Queue::write_buffer_with` refuses at once a write its buffer cannot take.

use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[test]
fn a_programs_gpu_errors_reach_it_as_wgpu_routes_them() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(
            Path::new(waos_test_programs::GPU_ERRORS),
            &waos_test_programs::code_of(waos_test_programs::GPU_ERRORS),
        )
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&program, printing_to(&output)).unwrap();
    let end = within(running.ended());
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "{end:?}: {:?}",
        output.lines()
    );
    assert_eq!(
        output.lines(),
        [
            "made after the inner scope's pop, the inner scope caught: none",
            "and the outer scope: validation",
            "made after a scope's guard was dropped, the handler heard: validation",
            "a device-lost callback replaced is dropped uncalled: true",
            "the callback set last heard: Some(Destroyed)",
            "a device dropped unlost drops its callback uncalled: true",
            "a write its buffer cannot take is refused at once: true",
        ]
    );
}
