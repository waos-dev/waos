//! `examples/gpu-calls` under the test shell: `wasdk:gpu` called by a program itself, each call
//! answered by the engine's GPU host as wgpu answers it. The adapter given with no preference is
//! the first offered; a device opens with the adapter's own limits, and a feature or a limit
//! past the adapter's, set by its setter, is refused; a buffer mapped at creation and one doubled by a compute pass
//! read back; a render pass draws; an error scope catches an error, and one no scope catches
//! reaches the device's uncaptured errors, which only the first take has; a destroyed device is
//! lost.

use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[test]
fn a_program_calling_the_gpu_itself_is_answered_as_wgpu_answers() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(
            Path::new(waos_test_programs::GPU_CALLS),
            &waos_test_programs::code_of(waos_test_programs::GPU_CALLS),
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
    let lines = output.lines();
    let expected = [
        "a limit past the adapter's: refused",
        "the device has the adapter's bind groups: true",
        "a later take of the uncaptured errors: none",
        "written at creation: [5, 6, 7, 8]",
        "doubled: [2, 4, 6, 8]",
        "drawn: [255, 0, 0, 255]",
        "caught by the scope: validation",
        "uncaptured: validation",
        "lost: destroyed",
    ];
    let (offered, lines_after) = lines.split_first().expect("the program printed");
    assert_eq!(
        offered, "the adapter given is the first offered: true",
        "{lines:?}"
    );
    let (feature, rest) = lines_after.split_first().expect("the program printed on");
    assert_eq!(feature, "a feature the adapter lacks: refused", "{lines:?}");
    assert_eq!(rest, expected, "{lines:?}");
}
