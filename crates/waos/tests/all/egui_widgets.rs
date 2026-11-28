//! `examples/egui-widgets` under the test shell: a program whose UI is egui, a view's events
//! reaching egui through `wasdk-adapter-egui` and egui drawn by its own wgpu renderer. It draws a
//! picture; a tap on its button, where the program says the button is, is a click to egui; and a
//! tap on its field starts the view's text input, through which what an input method types
//! reaches the field, as a key pressed then does.
//!
//! It draws, so it runs where the engine makes hardware buffers, on Apple's systems.

use std::path::Path;
use std::sync::Arc;

use waos::{Keyboard, ProgramEnd, ProgramView, RunningProgram, WaOS};

use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[test]
fn a_tap_on_its_button_is_a_click_and_text_and_keys_reach_its_field() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let (running, window) = run(&engine, &shell, &output);
    assert!(
        within(window.picture_after_events()),
        "egui's pass is drawn"
    );

    running.mouse().tap(&window, said_at(&output, "button"));
    output.wait_for("clicked 1");
    running.mouse().tap(&window, said_at(&output, "button"));
    output.wait_for("clicked 2");

    running.mouse().tap(&window, said_at(&output, "field"));
    shell
        .changes
        .wait_until("the field starts the view's text input", || {
            window.editing_value().is_some()
        });
    Keyboard::insert_text(&window, "milk").unwrap();
    output.wait_for("text: \"milk\"");
    // A key reaches the field too, which has the keyboard.
    Keyboard::press(&window, "Backspace").unwrap();
    output.wait_for("text: \"mil\"");

    window.close_requested();
    assert!(matches!(
        within(running.ended()),
        ProgramEnd::Returned(Ok(()))
    ));
}

/// The program, its output into `output`, and its window once it opened and said where its
/// widgets are.
fn run(
    engine: &WaOS,
    shell: &TestShell,
    output: &Arc<RecordedOutput>,
) -> (RunningProgram, ProgramView) {
    let program = engine
        .load(
            Path::new(waos_test_programs::EGUI_WIDGETS),
            &waos_test_programs::code_of(waos_test_programs::EGUI_WIDGETS),
        )
        .unwrap();
    let running = engine.run(&program, printing_to(output)).unwrap();
    let window = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("the program asks for its window");
    output.wait_for("window: open");
    output.wait_until("the program says where its widgets are", |lines| {
        lines.iter().any(|line| line.starts_with("field at "))
    });
    (running, window)
}

/// Where the program last said `widget` is, in its view's pixels.
fn said_at(output: &RecordedOutput, widget: &str) -> (f64, f64) {
    let said = format!("{widget} at ");
    let lines = output.lines();
    let place = lines
        .iter()
        .rev()
        .find_map(|line| line.strip_prefix(&said))
        .expect("the program said where the widget is");
    let (x, y) = place.split_once(',').expect("a place is x,y");
    (x.parse().unwrap(), y.parse().unwrap())
}
