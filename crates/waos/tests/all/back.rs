//! `examples/back` under the test shell: a program that goes back within itself. Its view starts
//! not handling Back; a tap takes it a page on, and the view, and the shell with it, hear that it
//! handles Back; the user's Back sent to it, as a shell sends it then, takes it a page back, and
//! back on its first page it says it no longer handles Back.

use std::path::Path;
use std::sync::Arc;

use waos::{ProgramEnd, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[test]
fn it_handles_back_while_it_has_a_page_to_go_back_to() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(
            Path::new(waos_test_programs::BACK),
            &waos_test_programs::code_of(waos_test_programs::BACK),
        )
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&program, printing_to(&output)).unwrap();
    let shown = within(shell.headless.view(0)).expect("the program asks for its window");
    let view = shown.view().expect("the program's view");
    output.wait_for("page 0, handles back: false");
    assert!(!view.handles_back(), "a view starts not handling Back");

    // The program says whether it handles Back before it prints the page it is on.
    running.mouse().tap(&view, (1.0, 1.0));
    output.wait_for("page 1, handles back: true");
    assert!(view.handles_back());
    assert!(shown.handles_back(), "the shell is told");
    running.mouse().tap(&view, (1.0, 1.0));
    output.wait_for("page 2, handles back: true");

    view.pop_route();
    // A Back queued behind one unread merges into it, as a shell's repeated Back does.
    output.wait_for("back");
    view.pop_route();
    // The program ends once it has heard every event before the request to close.
    view.close_requested();
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert_eq!(
        output.lines()[3..],
        [
            "back",
            "page 1, handles back: true",
            "back",
            "page 0, handles back: false"
        ],
        "each Back heard once"
    );
    assert!(!view.handles_back());
    assert!(!shown.handles_back(), "the shell is told");
}
