//! Drawing under the test shell: `examples/triangle` draws the triangle's colour at its middle and
//! the clear colour at a corner, read through `ProgramView::picture`; a frame due while the shell
//! holds every hardware buffer gets another, and the spares go once the view has drawn nothing for
//! a while; a triangle other windows cover draws only while a tool watches it, and not minimized,
//! watched or not; a view out of sight answers `occluded` for its surface's texture
//! (`examples/occluded`); a frozen triangle stops presenting, keeps one hardware buffer or two,
//! and draws again after the unfreeze; and `app_suspending` with triangle drawing resolves with
//! no GPU work in flight, so no frame reaches the shell after it.
//!
//! Hardware buffers are made on Apple's systems, where these tests run; elsewhere one test in their
//! place, in `main.rs`, says so and passes.
//! The tests of spares and of freezing move the engine's time (`EngineTime::Manual`) rather than
//! wait for it: a 300 ms delay, a freeze taking at a tick, every 100 ms, and spares going a second
//! after the view's last frame. A frozen triangle stands still once it runs nothing and its frames
//! in flight have landed (`RunningProgram::idle`): a freeze stops its next submission, so no frame
//! comes after that.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use waos::bindings::wasdk::platform::views::{
    Cursor, Placement, ViewFocusDirection, ViewFocusState,
};
use waos::{
    EngineTime, FreezePolicy, HardwareBuffer, ManualTime, Presence, ProgramEnd, ProgramEnvironment,
    ShellCompositionLayer, ShellView, WaOS, WaOSConfig,
};

use crate::test_shell::{
    DISPLAY, RecordedOutput, TestShell, WITHIN, config, pixel_of, place_view, printing_to,
    wait_until, within,
};

/// How long a program stays hidden before the engine freezes it, in the freezing tests.
const AFTER_HIDDEN: Duration = Duration::from_millis(300);

/// The engine's epoch tick, at which a freeze takes.
const TICK: Duration = Duration::from_millis(100);

const OFF_SCREEN: Presence = Presence {
    on_screen: false,
    occluded: false,
    focused: false,
};
const COVERED: Presence = Presence {
    on_screen: true,
    occluded: true,
    focused: false,
};
const FOCUSED: Presence = Presence {
    on_screen: true,
    occluded: false,
    focused: true,
};

/// Triangle's colour and its clear colour, as the bytes in a hardware buffer: blue, green, red,
/// alpha, of 0.95, 0.35, 0.25 and 0.06, 0.06, 0.08.
const TRIANGLE: [u8; 4] = [64, 89, 242, 255];
const CLEAR: [u8; 4] = [20, 15, 15, 255];

/// How far a byte may be from the colour's: a rounding of the GPU's.
const TOLERANCE: u8 = 2;

/// The bytes of one hardware buffer at the test shell's display size: BGRA8.
const HARDWARE_BUFFER_BYTES: u64 = DISPLAY.width as u64 * DISPLAY.height as u64 * 4;

/// How many frames the shell holds every hardware buffer of: more than the three a view starts
/// with and the four it once grew to at most.
const FRAMES_HELD: usize = 8;

/// How many hardware buffers a view keeps once it has drawn nothing for a while.
const STARTING_HARDWARE_BUFFERS: u64 = 3;

/// How long a view draws nothing before its spare hardware buffers go.
const SPARE_IDLE: Duration = Duration::from_secs(1);

fn assert_colour(what: &str, pixel: [u8; 4], colour: [u8; 4]) {
    let close = pixel
        .iter()
        .zip(colour)
        .all(|(byte, expected)| byte.abs_diff(expected) <= TOLERANCE);
    assert!(close, "{what} is {pixel:?}, not {colour:?}");
}

#[test]
fn a_presented_hardware_buffer_holds_the_triangle_over_the_clear_colour() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let triangle = engine
        .load(Path::new(waos_test_programs::TRIANGLE))
        .unwrap();
    let running = engine
        .run(&triangle, ProgramEnvironment::default())
        .unwrap();
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("triangle asks for its view");
    assert!(within(view.picture_after_events()), "its first picture");

    let picture = view.picture().unwrap().expect("a picture");
    assert_eq!(
        (picture.width, picture.height),
        (DISPLAY.width, DISPLAY.height)
    );
    let middle = pixel_of(
        &picture,
        DISPLAY.width as usize / 2,
        DISPLAY.height as usize / 2,
    );
    let corner = pixel_of(&picture, 0, 0);
    assert_colour("the middle of the picture", middle, TRIANGLE);
    assert_colour("the corner of the picture", corner, CLEAR);
    assert!(running.stats().frames_presented() >= 1);
    let buffers = running.stats().hardware_buffer_bytes() / HARDWARE_BUFFER_BYTES;
    assert!(
        (3..=4).contains(&buffers),
        "a drawing view holds three hardware buffers, a fourth while the shell holds all three, not {buffers}"
    );
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_frame_due_while_the_shell_holds_every_hardware_buffer_gets_another_and_the_spares_go_once_the_view_is_idle()
 {
    let time = ManualTime::new();
    let shell = TestShell::new();
    let held_view = Arc::new(HoldingBuffersView::default());
    let (placement, arrived) = place_view(Arc::clone(&held_view) as Arc<dyn ShellView>);
    let mut parts = shell.shell();
    parts.views = placement;
    let engine = WaOS::new(
        WaOSConfig {
            time: EngineTime::Manual(time.clone()),
            ..config()
        },
        parts,
        shell.platform(),
    )
    .unwrap();

    let triangle = engine
        .load(Path::new(waos_test_programs::TRIANGLE))
        .unwrap();
    let running = engine
        .run(&triangle, ProgramEnvironment::default())
        .unwrap();
    let view = arrived
        .recv_timeout(WITHIN)
        .expect("triangle asks for its view");

    wait_until("at least FRAMES_HELD frames are held", || {
        held_view.held_count() >= FRAMES_HELD
    });
    // Stop serving new frames so the view goes quiet (default freeze policy never freezes on hide)
    // and drain any frame already in flight before measuring the pool and advancing manual time.
    view.presence_changed(OFF_SCREEN);
    within(running.idle());
    let held = held_view.held_count();
    assert!(held >= FRAMES_HELD, "held {held} frames");
    assert_eq!(
        running.stats().hardware_buffer_bytes(),
        held as u64 * HARDWARE_BUFFER_BYTES,
        "each frame got a hardware buffer of its own, past the three a view starts with"
    );

    held_view.release_all_but_latest();
    time.elapse(SPARE_IDLE - TICK);
    assert_eq!(
        running.stats().hardware_buffer_bytes(),
        held as u64 * HARDWARE_BUFFER_BYTES,
        "the spares stay while the view has drawn nothing for less than a second"
    );
    time.elapse(TICK);
    assert_eq!(
        running.stats().hardware_buffer_bytes(),
        STARTING_HARDWARE_BUFFERS * HARDWARE_BUFFER_BYTES,
        "the spares go at the tick a second after the view's last frame"
    );
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_covered_triangle_draws_while_watched_and_not_minimized_nor_after_the_watch() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let triangle = engine
        .load(Path::new(waos_test_programs::TRIANGLE))
        .unwrap();
    let running = engine
        .run(&triangle, ProgramEnvironment::default())
        .unwrap();
    let shown = within(shell.headless.view(0)).expect("triangle asks for its view");
    let view = shown.view().expect("triangle's view");
    assert!(within(view.picture_after_events()), "triangle draws");

    // A second triangle on the same screen witnesses that refreshes keep arriving while the first
    // view is covered or minimized.
    let witness = engine
        .run(&triangle, ProgramEnvironment::default())
        .unwrap();
    let witness_shown = within(shell.headless.view(1)).expect("witness asks for its view");
    wait_until("witness draws", || witness_shown.pictures_shown() >= 1);
    let witness_draws = |frames: usize| {
        let before = witness_shown.pictures_shown();
        wait_until("witness keeps drawing", || {
            witness_shown.pictures_shown() >= before + frames
        });
    };

    // Covered and unwatched: draws nothing while the witness keeps drawing, then draws once watched.
    view.presence_changed(COVERED);
    within(running.idle());
    let presented = shown.pictures_shown();
    witness_draws(3);
    assert_eq!(
        shown.pictures_shown(),
        presented,
        "covered and unwatched, triangle draws nothing"
    );
    let watching = running.watch();
    wait_until("covered and watched, triangle draws", || {
        shown.pictures_shown() > presented
    });

    // Minimized while watched: draws nothing while the witness keeps drawing, then draws covered again.
    view.presence_changed(OFF_SCREEN);
    within(running.idle());
    let presented = shown.pictures_shown();
    witness_draws(3);
    assert_eq!(
        shown.pictures_shown(),
        presented,
        "minimized while watched, triangle draws nothing"
    );
    view.presence_changed(COVERED);
    wait_until("covered and watched again, triangle draws", || {
        shown.pictures_shown() > presented
    });

    // Covered after the watch ends: draws nothing while the witness keeps drawing, then draws once shown.
    drop(watching);
    within(running.idle());
    let presented = shown.pictures_shown();
    witness_draws(3);
    assert_eq!(
        shown.pictures_shown(),
        presented,
        "covered after the watch ends, triangle draws nothing"
    );
    view.presence_changed(FOCUSED);
    wait_until("shown again, triangle draws", || {
        shown.pictures_shown() > presented
    });

    witness.stop();
    running.stop();
    assert!(matches!(within(witness.ended()), ProgramEnd::Stopped));
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

/// `examples/occluded` takes its surface's texture at each change of its state: a frame in sight,
/// `occluded` covered or minimized, as wgpu's surfaces answer, and a frame again once shown.
#[test]
fn a_view_out_of_sight_answers_occluded_for_its_surfaces_texture() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let occluded = engine
        .load(Path::new(waos_test_programs::OCCLUDED))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&occluded, printing_to(&output)).unwrap();
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("occluded asks for its view");
    let said = |line: &'static str| {
        wait_until(line, || output.lines().iter().any(|said| said == line));
    };
    said("started: a frame");
    view.presence_changed(COVERED);
    said("hidden: occluded");
    view.presence_changed(FOCUSED);
    said("resumed: a frame");
    view.presence_changed(OFF_SCREEN);
    wait_until("minimized, occluded again", || {
        output
            .lines()
            .iter()
            .filter(|said| *said == "hidden: occluded")
            .count()
            == 2
    });
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

/// An engine freezing a program [`AFTER_HIDDEN`] out of sight, on time the test moves.
fn freezing_engine() -> (WaOS, TestShell, ManualTime) {
    let time = ManualTime::new();
    let shell = TestShell::new();
    let engine = WaOS::new(
        WaOSConfig {
            freeze: FreezePolicy {
                freeze_hidden_after: Some(AFTER_HIDDEN),
                ..FreezePolicy::default()
            },
            time: EngineTime::Manual(time.clone()),
            ..config()
        },
        shell.shell(),
        shell.platform(),
    )
    .unwrap();
    (engine, shell, time)
}

#[test]
fn a_frozen_triangle_keeps_one_hardware_buffer_and_draws_again_after_the_unfreeze() {
    let (engine, shell, time) = freezing_engine();
    let triangle = engine
        .load(Path::new(waos_test_programs::TRIANGLE))
        .unwrap();
    let running = engine
        .run(&triangle, ProgramEnvironment::default())
        .unwrap();
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("triangle asks for its view");
    assert!(within(view.picture_after_events()), "triangle is drawing");

    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN);
    within(running.idle());
    let presented = running.stats().frames_presented();
    time.elapse(AFTER_HIDDEN);
    within(running.idle());
    assert_eq!(
        running.stats().frames_presented(),
        presented,
        "frozen, it presents nothing"
    );
    let frozen = running.stats().hardware_buffer_bytes() / HARDWARE_BUFFER_BYTES;
    assert!(
        frozen <= 2,
        "frozen, the view keeps the hardware buffer on screen and at most the one being drawn, not {frozen}"
    );

    view.presence_changed(FOCUSED);
    wait_until("unfrozen, triangle draws again", || {
        running.stats().frames_presented() > presented
    });
    let unfrozen = running.stats().hardware_buffer_bytes() / HARDWARE_BUFFER_BYTES;
    assert!(
        unfrozen >= 2,
        "unfrozen, the view has a hardware buffer to draw into beside the one on screen, not {unfrozen}"
    );
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn app_suspending_with_triangle_drawing_resolves_with_no_gpu_work_in_flight() {
    let (engine, shell, time) = freezing_engine();
    let triangle = engine
        .load(Path::new(waos_test_programs::TRIANGLE))
        .unwrap();
    let running = engine
        .run(&triangle, ProgramEnvironment::default())
        .unwrap();
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("triangle asks for its view");
    assert!(within(view.picture_after_events()), "triangle is drawing");

    within(engine.suspend());
    let presented = running.stats().frames_presented();
    time.elapse(AFTER_HIDDEN);
    within(running.idle());
    assert_eq!(
        running.stats().frames_presented(),
        presented,
        "no frame reaches the shell after the app suspended: none was in flight"
    );

    engine.resume();
    wait_until("resumed, triangle draws again", || {
        running.stats().frames_presented() > presented
    });
    running.stop();
    assert!(matches!(within(running.ended()), ProgramEnd::Stopped));
}

/// A view that keeps every hardware buffer presented to it until the test releases them: used only
/// by `a_frame_due_while_the_shell_holds_every_hardware_buffer_gets_another_and_the_spares_go_once_the_view_is_idle`
/// to exercise pool growth beyond the starting buffers, which `HeadlessView` does not retain.
#[derive(Default)]
struct HoldingBuffersView {
    held: Mutex<Vec<Box<dyn FnOnce() + Send + Sync>>>,
}

impl HoldingBuffersView {
    fn held_count(&self) -> usize {
        self.held.lock().unwrap().len()
    }

    /// Releases every hardware buffer held but the one presented last.
    fn release_all_but_latest(&self) {
        let released: Vec<_> = {
            let mut held = self.held.lock().unwrap();
            let last = held.len().saturating_sub(1);
            held.drain(..last).collect()
        };
        for release in released {
            release();
        }
    }
}

impl ShellView for HoldingBuffersView {
    fn present(
        &self,
        _buffer: HardwareBuffer,
        _layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        self.held.lock().unwrap().push(released);
    }

    fn set_cursor(&self, _cursor: Cursor) {}
    fn request_focus_change(&self, _state: ViewFocusState, _direction: ViewFocusDirection) {}
    fn set_handles_back(&self, _handles: bool) {}
}
