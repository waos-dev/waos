//! `examples/readback` under the test shell: a buffer read back from the GPU both ways wgpu
//! programs do. Code written for native wgpu asks, calls `Device::poll` with `Wait`, and reads at
//! once, so the mapping's callback, and the queue's submitted-work callback asked after it, have
//! run before `poll` returns; code written for the web asks and awaits. A program that polled and
//! then read hung before, its callback waiting for an `.await` that never came.
//!
//! And `examples/naga-shader`: a shader given as naga's module, not as WGSL text, is written as
//! WGSL on its way to the engine, and runs; one naga's validation refuses makes a validation
//! error.

use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[test]
fn a_buffer_is_read_back_by_polling_and_by_awaiting() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::READBACK))
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
            "polled: [1, 2, 3, 4, 5, 6, 7, 8]",
            "submitted work done: told before poll returned",
            "awaited: [1, 2, 3, 4, 5, 6, 7, 8]",
        ]
    );
}

#[test]
fn a_shader_given_as_nagas_module_runs() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::NAGA_SHADER))
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
            "compilation messages: 0",
            "doubled: [2, 4, 6, 8]",
            "an invalid module, the scope caught: validation",
        ]
    );
}
