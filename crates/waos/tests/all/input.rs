//! `examples/input` under the test shell, read as a tool that looks at a program's window and
//! drives it reads it: its window carries the title it asked for, and its picture is the last one
//! it presented, in the colour it drew; a press reaches it as the pointer events a mouse gives, and
//! the picture the program presented after reading them is the next colour; once its first press
//! starts editing, what it edits is the engine's copy, which an input method's change replaces; and
//! once its window is gone, a wait for a picture ends without one and there is none to read.
//!
//! It draws, so it runs where the engine makes hardware buffers, on Apple's systems.

use std::path::Path;
use std::sync::Arc;

use waos::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
use waos::bindings::wasdk::platform::views::{
    PointerButtons, PointerChange, PointerDeviceKind, PointerEvent, PointerSignalKind,
};
use waos::{HeldModifiers, Keyboard, ProgramEnd, ProgramView, RunningProgram, WaOS};

use crate::test_shell::{
    DISPLAY, RecordedOutput, TestShell, config, pixel_of, printing_to, wait_until, within,
};

/// The program's first two colours, as the bytes of a hardware buffer: blue, green, red, alpha,
/// of 0.1, 0.1, 0.4 and 0.9, 0.5, 0.1.
const FIRST: [u8; 4] = [102, 26, 26, 255];
const SECOND: [u8; 4] = [26, 128, 230, 255];

/// How far a byte may be from the colour's: a rounding of the GPU's.
const TOLERANCE: u8 = 2;

#[test]
fn its_window_is_titled_as_asked_and_its_picture_is_the_colour_it_drew() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let (running, window) = run(&engine, &shell, &output);
    assert_eq!(window.window_title().as_deref(), Some("Input"));
    assert!(within(window.picture_after_events()), "its first picture");
    let picture = window.picture().unwrap().expect("a picture");
    assert_eq!(
        (picture.width, picture.height),
        (DISPLAY.width, DISPLAY.height)
    );
    assert_colour("the middle", middle(&picture), FIRST);
    assert_colour("a corner", pixel_of(&picture, 0, 0), FIRST);
    end(&running, &window);
}

#[test]
fn a_press_reaches_it_and_the_picture_after_it_is_the_next_colour() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let (running, window) = run(&engine, &shell, &output);
    assert!(within(window.picture_after_events()), "its first picture");

    press(&window, 40.0, 30.0);
    let after_press = window.picture_after_events();
    release(&window, 40.0, 30.0);
    output.wait_for(
        "pointer: PointerChange::Down PointerDeviceKind::Mouse PointerSignalKind::None device 7 at 40,30 buttons 1 pressure 1 scroll 0,0",
    );
    assert!(within(after_press), "the picture after the press");
    assert_colour(
        "the picture after the press",
        middle(&window.picture().unwrap().unwrap()),
        SECOND,
    );
    end(&running, &window);
}

#[test]
fn what_it_edits_is_the_engines_copy_which_an_input_methods_change_replaces() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let (running, window) = run(&engine, &shell, &output);
    assert!(window.editing_value().is_none(), "nothing edited before");
    running.mouse().tap(&window, (1.0, 1.0));
    output.wait_for("text: editing");
    wait_until("the program's empty text is kept", || {
        window
            .editing_value()
            .is_some_and(|value| value.text.is_empty())
    });

    Keyboard::insert_text(&window, "milk").unwrap();
    output.wait_for("text: \"milk\" selection Some(\"4..4\")");
    assert_eq!(window.editing_value().unwrap().text, "milk");
    end(&running, &window);
}

#[test]
fn once_its_window_is_gone_a_wait_ends_without_a_picture_and_there_is_none_to_read() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let (running, window) = run(&engine, &shell, &output);
    assert!(within(window.picture_after_events()), "its first picture");
    running.remove_view(window.id());
    assert!(!within(window.picture_after_events()));
    assert!(window.picture().unwrap().is_none());
    assert!(matches!(
        within(running.ended()),
        ProgramEnd::Returned(Ok(()))
    ));
}

/// The program, its output into `output`, and its window once it opened.
fn run(
    engine: &WaOS,
    shell: &TestShell,
    output: &Arc<RecordedOutput>,
) -> (RunningProgram, ProgramView) {
    let program = engine.load(Path::new(waos_test_programs::INPUT)).unwrap();
    let running = engine.run(&program, printing_to(output)).unwrap();
    let window = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("the program asks for its window");
    output.wait_for("window: open");
    (running, window)
}

/// Asks the program to close its window, and waits for it to end with its view.
fn end(running: &RunningProgram, window: &ProgramView) {
    window.close_requested();
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}

/// The mouse coming to `(x, y)` in `window` and its button going down there.
fn press(window: &ProgramView, x: f64, y: f64) {
    let held = HeldModifiers::default();
    for (change, buttons) in [
        (PointerChange::Add, PointerButtons::empty()),
        (PointerChange::Hover, PointerButtons::empty()),
        (PointerChange::Down, PointerButtons::PRIMARY),
    ] {
        window.pointer(mouse_event(change, buttons, x, y), &held);
    }
}

/// The mouse's button coming up at `(x, y)` in `window`.
fn release(window: &ProgramView, x: f64, y: f64) {
    let up = mouse_event(PointerChange::Up, PointerButtons::empty(), x, y);
    window.pointer(up, &HeldModifiers::default());
}

fn mouse_event(change: PointerChange, buttons: PointerButtons, x: f64, y: f64) -> PointerEvent {
    PointerEvent {
        time: 0,
        change,
        kind: PointerDeviceKind::Mouse,
        signal: PointerSignalKind::None,
        device: 7,
        x,
        y,
        buttons,
        pressure: 1.0,
        pressure_min: 1.0,
        pressure_max: 1.0,
        distance: 0.0,
        distance_max: 0.0,
        size: 0.0,
        radius_major: 0.0,
        radius_minor: 0.0,
        radius_min: 0.0,
        radius_max: 0.0,
        orientation: 0.0,
        tilt: 0.0,
        scroll_delta_x: 0.0,
        scroll_delta_y: 0.0,
        pan_x: 0.0,
        pan_y: 0.0,
        scale: 1.0,
        rotation: 0.0,
    }
}

fn middle(picture: &PixelBuffer) -> [u8; 4] {
    pixel_of(
        picture,
        picture.width as usize / 2,
        picture.height as usize / 2,
    )
}

fn assert_colour(what: &str, pixel: [u8; 4], colour: [u8; 4]) {
    let close = pixel
        .iter()
        .zip(colour)
        .all(|(byte, expected)| byte.abs_diff(expected) <= TOLERANCE);
    assert!(close, "{what} is {pixel:?}, not {colour:?}");
}
