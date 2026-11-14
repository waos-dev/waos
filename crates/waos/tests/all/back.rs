//! `examples/back` under the test shell: a program that goes back within itself. Its view starts
//! not handling Back; a press takes it a page on, and the view, and the shell with it, hear that it
//! handles Back; the user's Back sent to it, as a shell sends it then, takes it a page back, and
//! back on its first page it says it no longer handles Back, the shell told each change once.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use waos::bindings::wasdk::platform::views::{PointerButtons, PointerChange};
use waos::{HeldModifiers, ProgramEnvironment, ProgramOutput, ProgramView, RunningProgram, WaOS};

use crate::shell::{self, Lines, WITHIN, mouse, wait_until};

#[test]
fn it_handles_back_while_it_has_a_page_to_go_back_to() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let (running, view) = run(&engine, &arrivals, &lines);
    let shell_view = Arc::clone(&shell.placed_views()[0]);
    assert!(!view.handles_back(), "a view starts not handling Back");

    press(&view);
    lines.wait_for("page 1, handles back: true");
    wait_until("the view handles Back", || view.handles_back());
    press(&view);
    lines.wait_for("page 2, handles back: true");

    view.pop_route();
    wait_until("back on page 1", || {
        lines.count("page 1, handles back: true") == 2
    });
    view.pop_route();
    lines.wait_for("page 0, handles back: false");
    wait_until("the view no longer handles Back", || !view.handles_back());
    assert_eq!(lines.count("back"), 2, "each Back heard once");
    assert_eq!(shell_view.handles_back(), [true, false], "each change once");
    end(&running, &view);
}

/// The program, its output into `lines`, and its window once it opened.
fn run(
    engine: &WaOS,
    arrivals: &Receiver<ProgramView>,
    lines: &Arc<Lines>,
) -> (RunningProgram, ProgramView) {
    let program = shell::load(engine, waos_test_programs::BACK);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(lines) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let (running, _given) = engine.run(&program, environment, Vec::new()).unwrap();
    let view = arrivals
        .recv_timeout(WITHIN)
        .expect("the program asks for its window");
    lines.wait_for("page 0, handles back: false");
    (running, view)
}

/// A mouse's press at the view's corner.
fn press(view: &ProgramView) {
    let held = HeldModifiers::default();
    view.pointer(
        mouse(PointerChange::Down, PointerButtons::PRIMARY, (1.0, 1.0), 0),
        &held,
    );
}

/// Asks the program to close its window, and waits for it to end with its view.
fn end(running: &RunningProgram, view: &ProgramView) {
    view.close_requested();
    running.remove_view(view.id());
    shell::wait_for_end(running);
}
