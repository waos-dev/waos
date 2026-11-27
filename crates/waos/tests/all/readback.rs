//! `examples/readback` under the test shell: a buffer read back from the GPU as native wgpu
//! programs do. The program asks, calls `Device::poll` with `Wait`, and reads at once, so the
//! mapping's callback, and the queue's submitted-work callback asked after it, have run before
//! `poll` returns. A program that polled and then read hung before, its callback waiting for an
//! `.await` that never came. Each request
//! reaches the GPU at the call: an `unmap` before a mapping is answered aborts it, and `poll` with
//! `Poll` answers what is done. A buffer mapped for writing keeps what was written between
//! mappings, though its bytes are never read back from the engine.
//!
//! And `examples/naga-shader`: a shader given as naga's module, not as WGSL text, is written as
//! WGSL on its way to the engine, and runs; one naga's validation refuses makes a validation
//! error.

use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[test]
fn a_buffer_is_read_back_by_polling() {
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
            "work done before a later submission: told once the earlier was",
            "unmapped before mapped: aborted, and maps again",
            "polled without waiting: answered",
            "polled through the instance: answered",
            "written in part: [9, 9, 9, 9, 5, 6, 7, 8]",
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
