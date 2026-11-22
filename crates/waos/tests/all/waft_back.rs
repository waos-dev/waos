//! `examples/waft-back`, a Waft app over `wasdk-adapter-waft` with pages to go back through, under
//! the test shell: it says nothing of Back on its first page; a tap pushes a page onto its
//! navigator, and its view, and the shell with it, hear that it handles Back, with no code of the
//! app's own; the user's Back sent to it pops the page, and back on its first page it no longer
//! handles Back. It is a debug build of Waft, slow to start, and draws, so it runs where the
//! engine makes hardware buffers, on Apple's systems.

use std::sync::Arc;

use waos::bindings::wasdk::platform::views::{PointerButtons, PointerChange};
use waos::{HeldModifiers, ProgramEnd, ProgramEnvironment, ProgramOutput, ProgramView};

use crate::shell::{self, Lines, WITHIN, mouse, wait_for_presents, wait_until};

#[test]
fn its_navigator_handles_back_while_it_can_pop() {
    let (engine, shell, arrivals) = shell::engine();
    let lines = Arc::new(Lines::default());
    let program = shell::load(&engine, waos_test_programs::WAFT_BACK);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        ..waos_local::LocalCapabilities::new().environment()
    };
    let running = engine.run(&program, environment).unwrap();
    let view = arrivals
        .recv_timeout(WITHIN)
        .expect("waft-back asks for its window");
    let shell_view = Arc::clone(&shell.placed_views()[0]);
    wait_for_presents(&running, &shell_view, 1, || lines.all());
    lines.wait_for("page 0");
    assert!(
        !view.handles_back(),
        "nothing to go back to on its first page"
    );

    click(&view);
    lines.wait_for("page 1");
    wait_until("the app handles Back", || view.handles_back());

    view.pop_route();
    wait_until("the app no longer handles Back", || !view.handles_back());
    assert_eq!(shell_view.handles_back(), [true, false]);

    view.close_requested();
    let end = shell::wait_for_end(&running);
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

/// A mouse's click in the middle of `view`.
fn click(view: &ProgramView) {
    let held = HeldModifiers::default();
    let middle = (100.0, 100.0);
    for (change, buttons) in [
        (PointerChange::Add, PointerButtons::empty()),
        (PointerChange::Hover, PointerButtons::empty()),
        (PointerChange::Down, PointerButtons::PRIMARY),
        (PointerChange::Up, PointerButtons::empty()),
    ] {
        view.pointer(mouse(change, buttons, middle, 0), &held);
    }
}
