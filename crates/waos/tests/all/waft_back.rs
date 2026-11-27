//! `examples/waft-back`, a Waft app over `wasdk-adapter-waft` with pages to go back through, under
//! the test shell: it says nothing of Back on its first page; a tap pushes a page onto its
//! navigator, and its view, and the shell with it, hear that it handles Back, with no code of the
//! app's own; the user's Back sent to it pops the page, and back on its first page it no longer
//! handles Back. It is a debug build of Waft, slow to start, and draws, so it runs where the
//! engine makes hardware buffers, on Apple's systems.

use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, ProgramEnvironment, ProgramOutput, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, wait_until, within};

#[test]
fn its_navigator_handles_back_while_it_can_pop() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(
            Path::new(waos_test_programs::WAFT_BACK),
            &waos_test_programs::code_of(waos_test_programs::WAFT_BACK),
        )
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&output) as Arc<dyn ProgramOutput>),
        ..waos_local::LocalCapabilities::new().environment()
    };
    let running = engine.run(&program, environment).unwrap();
    let shown = within(shell.headless.view(0)).expect("waft-back asks for its window");
    let view = shown.view().expect("waft-back's view");
    assert!(
        within(view.picture_after_events()),
        "waft-back draws its first page"
    );
    output.wait_for("page 0");
    assert!(
        !view.handles_back(),
        "nothing to go back to on its first page"
    );

    running.mouse().tap(&view, (100.0, 100.0));
    output.wait_for("page 1");
    wait_until("the app handles Back", || shown.handles_back());
    assert!(view.handles_back());

    view.pop_route();
    wait_until("the app no longer handles Back", || !shown.handles_back());
    assert!(!view.handles_back());

    view.close_requested();
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}
