//! `examples/input` under the test shell, read as a tool that looks at a program's window and
//! drives it reads it (the design's decision 60): its window carries the title it asked for, and
//! its picture is the last one it presented, in the colour it drew; a press reaches it as the
//! pointer events a mouse gives, and the picture the program presented after reading them is the
//! next colour, held back until the shell takes it; once its first press starts editing, what it
//! edits is the engine's copy, which an input method's change replaces; and once its window is
//! gone, a wait for a picture ends without one and there is none to read.
//!
//! It draws, so it runs where the engine makes hardware buffers, on Apple's systems.

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, mpsc};

use waos::bindings::wasdk::platform::hardware_buffers::PixelBuffer;
use waos::bindings::wasdk::platform::views::{
    Cursor, OverlayStyle, Placement, PointerButtons, PointerChange, PointerDeviceKind,
    PointerEvent, PointerSignalKind, ViewError, ViewFocusDirection, ViewFocusState,
};
use waos::{
    HardwareBuffer, HeldModifiers, Keyboard, PlacedView, Presence, ProgramEnd, ProgramView,
    RunningProgram, ShellCompositionLayer, ShellView, ViewPlacement, ViewRequest, WaOS,
};
use waos_headless::SCREEN;

use crate::test_shell::{
    DISPLAY, RecordedOutput, TestShell, WITHIN, config, metrics, pixel_of, printing_to, wait_until,
    within,
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
fn a_press_reaches_it_and_the_picture_after_it_is_held_until_the_shell_takes_it() {
    let shell = TestShell::new();
    let held_view = Arc::new(HoldingView::default());
    let (arrivals, arrived) = mpsc::channel();
    let placement = Arc::new(HoldingPlacement {
        view: Arc::clone(&held_view),
        arrivals,
    });
    let mut parts = shell.shell();
    parts.views = placement;
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let program = engine.load(Path::new(waos_test_programs::INPUT)).unwrap();
    let running = engine.run(&program, printing_to(&output)).unwrap();
    let window = arrived
        .recv_timeout(WITHIN)
        .expect("the program asks for its window");
    output.wait_for("window: open");
    assert!(within(window.picture_after_events()), "its first picture");

    held_view.hold_presents();
    press(&window, 40.0, 30.0);
    let after_press = window.picture_after_events();
    release(&window, 40.0, 30.0);
    output.wait_for(
        "pointer: PointerChange::Down PointerDeviceKind::Mouse PointerSignalKind::None device 7 at 40,30 buttons 1 pressure 1 scroll 0,0",
    );
    assert_colour(
        "the picture before the press is taken",
        middle(&window.picture().unwrap().unwrap()),
        FIRST,
    );
    held_view.let_presents_through();
    assert!(within(after_press), "the picture after the press");
    assert_colour(
        "the picture after the press",
        middle(&window.picture().unwrap().unwrap()),
        SECOND,
    );
    running.remove_view(window.id());
    assert!(matches!(
        within(running.ended()),
        ProgramEnd::Returned(Ok(()))
    ));
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

/// A view whose presents a test can hold back, as GPU work or a shell that has not taken a frame
/// yet holds them.
#[derive(Default)]
struct HoldingView {
    presents_held: Mutex<bool>,
    presents_let_through: Condvar,
    shown: Mutex<Option<Box<dyn FnOnce() + Send + Sync>>>,
}

impl HoldingView {
    fn hold_presents(&self) {
        *self.presents_held.lock().unwrap() = true;
    }

    fn let_presents_through(&self) {
        *self.presents_held.lock().unwrap() = false;
        self.presents_let_through.notify_all();
    }
}

impl ShellView for HoldingView {
    fn present(
        &self,
        _buffer: HardwareBuffer,
        _layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        let held = self.presents_held.lock().unwrap();
        let _through = self
            .presents_let_through
            .wait_while(held, |held| *held)
            .unwrap();
        let before = self.shown.lock().unwrap().replace(released);
        if let Some(before) = before {
            before();
        }
    }

    fn set_cursor(&self, _cursor: Cursor) {}
    fn request_focus_change(&self, _state: ViewFocusState, _direction: ViewFocusDirection) {}
    fn set_handles_back(&self, _handles: bool) {}
    fn set_overlay_style(&self, _style: OverlayStyle) {}
}

/// Places a single [`HoldingView`] on the test shell's screen and hands the [`ProgramView`] back.
struct HoldingPlacement {
    view: Arc<HoldingView>,
    arrivals: mpsc::Sender<ProgramView>,
}

impl ViewPlacement for HoldingPlacement {
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError> {
        let view = program.add_view(
            request.id,
            PlacedView {
                view: Arc::clone(&self.view) as Arc<dyn ShellView>,
                text_input: None,
                window: None,
                metrics: metrics(DISPLAY.width, DISPLAY.height),
                screen: Some(SCREEN),
                presence: Presence {
                    on_screen: true,
                    occluded: false,
                    focused: true,
                },
            },
        );
        let _ = self.arrivals.send(view);
        Ok(())
    }
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
