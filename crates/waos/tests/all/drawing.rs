//! Drawing under the test shell: `examples/triangle` presents frames to the shell's view, whose
//! screen refreshes as soon as it is asked and which releases each hardware buffer when the next
//! replaces it; a presented hardware buffer holds the triangle's colour at its middle and the clear
//! colour at a corner, read back by locking the `IOSurface`; a frame due while the shell holds
//! every hardware buffer gets another, and the spares go once the view has drawn nothing for a
//! while; a frozen triangle stops presenting, keeps one hardware buffer or two, and draws again
//! after the thaw; a triangle other windows cover draws only while a tool watches it, and not
//! minimized, watched or not; and `app_suspending` with triangle drawing resolves with no GPU work
//! in flight, so no frame reaches the shell after it.
//!
//! Hardware buffers are made on Apple's systems, where these tests run; elsewhere one test in their
//! place, in `main.rs`, says so and passes.
//! The tests of spares and of freezing move the engine's time (`EngineTime::Manual`) rather than
//! wait for it: a 300 ms delay, a freeze taking at a tick, every 100 ms, and spares going a second
//! after the view's last frame. A frozen triangle stands still once it runs nothing and its frames
//! in flight have landed (`RunningProgram::idle`): a freeze stops its next submission, so no frame
//! comes after that.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use waos::{
    FreezePolicy, ManualTime, Presence, ProgramEnd, ProgramEnvironment, ProgramView,
    RunningProgram, WaOS,
};

use crate::shell::hardware_buffer::pixel;
use crate::shell::{self, RecordingView, TestShell, VIEW_HEIGHT, VIEW_WIDTH, wait_until_idle};

/// How long a program stays hidden before the engine freezes it, in the freezing tests.
const AFTER_HIDDEN: Duration = Duration::from_millis(300);

/// The engine's epoch tick, at which a freeze takes.
const TICK: Duration = Duration::from_millis(100);

/// How long without a frame counts as having stopped drawing: many frames' time.
const QUIET: Duration = Duration::from_millis(400);

const OFF_SCREEN: Presence = Presence {
    on_screen: false,
    occluded: false,
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

/// The bytes of one hardware buffer at the shell's view size: BGRA8.
const HARDWARE_BUFFER_BYTES: u64 = VIEW_WIDTH as u64 * VIEW_HEIGHT as u64 * 4;

/// How many frames, at least, the shell holds every hardware buffer of: more than the three a
/// view starts with and the four it once grew to at most.
const FRAMES_HELD: usize = 8;

/// How many hardware buffers a view keeps once it has drawn nothing for a while.
const STARTING_HARDWARE_BUFFERS: u64 = 3;

/// How long a view draws nothing before its spare hardware buffers go.
const SPARE_IDLE: Duration = Duration::from_secs(1);

/// Triangle running on the one view it asks for, which `arrivals` hears, and the shell's side of
/// that view, with `before_run` done to it before the program has it.
fn run_triangle(
    engine: &WaOS,
    shell: &TestShell,
    arrivals: &Receiver<ProgramView>,
    before_run: impl FnOnce(&RecordingView) + Send + 'static,
) -> (RunningProgram, ProgramView, Arc<RecordingView>) {
    shell.prepare_next_view(before_run);
    let triangle = shell::load(engine, waos_test_programs::TRIANGLE);
    let (running, view) =
        shell::run_shown(engine, arrivals, &triangle, ProgramEnvironment::default());
    let shell_view = shell.placed_views().pop().expect("the view was placed");
    (running, view, shell_view)
}

/// Waits until no frame has been presented for [`QUIET`], and answers how many were.
fn presents_before_quiet(shell_view: &RecordingView) -> usize {
    let mut seen = shell_view.presents();
    let mut last_at = Instant::now();
    loop {
        thread::sleep(Duration::from_millis(10));
        let presents = shell_view.presents();
        if presents != seen {
            seen = presents;
            last_at = Instant::now();
        } else if last_at.elapsed() >= QUIET {
            return seen;
        }
    }
}

fn assert_colour(what: &str, pixel: [u8; 4], colour: [u8; 4]) {
    let close = pixel
        .iter()
        .zip(colour)
        .all(|(byte, expected)| byte.abs_diff(expected) <= TOLERANCE);
    assert!(close, "{what} is {pixel:?}, not {colour:?}");
}

#[test]
fn triangle_presents_frames_the_shell_shows_and_releases() {
    let (engine, shell, arrivals) = shell::engine();
    let started = Instant::now();
    let (running, _view, shell_view) = run_triangle(&engine, &shell, &arrivals, |_| {});
    shell_view.wait_for_presents(1);
    let first_frame = started.elapsed();
    let counted = Instant::now();
    shell_view.wait_for_presents(61);
    let sixty_frames = counted.elapsed();
    eprintln!(
        "triangle: the first frame reached the shell {first_frame:?} after the start; the next sixty took {sixty_frames:?}"
    );
    assert!(running.stats().frames_presented() >= 61);
    let buffers = running.stats().hardware_buffer_bytes() / HARDWARE_BUFFER_BYTES;
    assert!(
        (3..=4).contains(&buffers),
        "a drawing view holds three hardware buffers, a fourth while the shell holds all three, not {buffers}"
    );
    assert!(
        shell_view.hardware_buffers_held() <= 1,
        "each hardware buffer is released when the next replaces it"
    );
    assert!(
        shell.refreshes_asked() >= shell_view.presents(),
        "every frame was asked for"
    );
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_presented_hardware_buffer_holds_the_triangle_over_the_clear_colour() {
    let (engine, shell, arrivals) = shell::engine();
    let (running, _view, shell_view) = run_triangle(&engine, &shell, &arrivals, |_| {});
    shell_view.wait_for_presents(4);
    // Once the refreshes stop and the frames in flight have landed, the hardware buffer
    // presented last is at rest with the GPU done with it.
    shell.stop_refreshes();
    presents_before_quiet(&shell_view);
    let buffer = shell_view
        .latest_hardware_buffer()
        .expect("a hardware buffer is held");
    let middle = pixel(&buffer, VIEW_WIDTH as usize / 2, VIEW_HEIGHT as usize / 2);
    let corner = pixel(&buffer, 0, 0);
    eprintln!("triangle: the middle pixel is {middle:?} and the corner {corner:?}, as BGRA");
    assert_colour("the middle of the hardware buffer", middle, TRIANGLE);
    assert_colour("the corner of the hardware buffer", corner, CLEAR);
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_frame_due_while_the_shell_holds_every_hardware_buffer_gets_another_and_the_spares_go_once_the_view_is_idle()
 {
    let time = ManualTime::new();
    let (engine, shell, arrivals) = shell::engine_on(&time, FreezePolicy::default());
    let (running, _view, shell_view) = run_triangle(
        &engine,
        &shell,
        &arrivals,
        RecordingView::hold_every_hardware_buffer,
    );
    shell_view.wait_for_presents(FRAMES_HELD);
    shell.stop_refreshes();
    let presented = presents_before_quiet(&shell_view);
    assert!(presented >= FRAMES_HELD);
    assert_eq!(shell_view.hardware_buffers_held(), presented);
    assert_eq!(
        running.stats().hardware_buffer_bytes(),
        presented as u64 * HARDWARE_BUFFER_BYTES,
        "each frame got a hardware buffer of its own, past the three a view starts with"
    );

    shell_view.release_all_but_latest();
    time.elapse(SPARE_IDLE - TICK);
    assert_eq!(
        running.stats().hardware_buffer_bytes(),
        presented as u64 * HARDWARE_BUFFER_BYTES,
        "the spares stay while the view has drawn nothing for less than a second"
    );
    time.elapse(TICK);
    assert_eq!(
        running.stats().hardware_buffer_bytes(),
        STARTING_HARDWARE_BUFFERS * HARDWARE_BUFFER_BYTES,
        "the spares go at the tick a second after the view's last frame"
    );
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn a_covered_triangle_draws_while_watched_and_not_minimized_nor_after_the_watch() {
    let (engine, shell, arrivals) = shell::engine();
    let (running, view, shell_view) = run_triangle(&engine, &shell, &arrivals, |_| {});
    shell_view.wait_for_presents(1);

    shell::cover(&view);
    let covered = presents_before_quiet(&shell_view);
    let watching = running.watch();
    shell_view.wait_for_presents(covered + 10);

    shell::hide(&view);
    let minimized = presents_before_quiet(&shell_view);
    shell::cover(&view);
    shell_view.wait_for_presents(minimized + 10);

    drop(watching);
    let unwatched = presents_before_quiet(&shell_view);
    thread::sleep(QUIET);
    assert_eq!(
        shell_view.presents(),
        unwatched,
        "covered and no longer watched, it draws nothing"
    );
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
}

/// An engine freezing a program [`AFTER_HIDDEN`] out of sight, on time the test moves.
fn freezing_engine() -> (Arc<WaOS>, Arc<TestShell>, Receiver<ProgramView>, ManualTime) {
    let time = ManualTime::new();
    let (engine, shell, arrivals) = shell::engine_on(
        &time,
        FreezePolicy {
            after_hidden: Some(AFTER_HIDDEN),
        },
    );
    (engine, shell, arrivals, time)
}

#[test]
fn a_frozen_triangle_keeps_one_hardware_buffer_and_draws_again_after_the_thaw() {
    let (engine, shell, arrivals, time) = freezing_engine();
    let (running, view, shell_view) = run_triangle(&engine, &shell, &arrivals, |_| {});
    shell_view.wait_for_presents(30);
    let drawing = running.stats().hardware_buffer_bytes() / HARDWARE_BUFFER_BYTES;

    view.presence_changed(OFF_SCREEN);
    time.elapse(AFTER_HIDDEN);
    wait_until_idle(&running);
    let presented = shell_view.presents();
    time.elapse(AFTER_HIDDEN);
    wait_until_idle(&running);
    assert_eq!(
        shell_view.presents(),
        presented,
        "frozen, it presents nothing"
    );
    let frozen = running.stats().hardware_buffer_bytes() / HARDWARE_BUFFER_BYTES;
    assert!(
        frozen <= 2,
        "frozen, the view keeps the hardware buffer on screen and at most the one being drawn, not {frozen}"
    );

    view.presence_changed(FOCUSED);
    shell_view.wait_for_presents(presented + 30);
    let thawed = running.stats().hardware_buffer_bytes() / HARDWARE_BUFFER_BYTES;
    eprintln!("triangle: {drawing} hardware buffers drawing, {frozen} frozen, {thawed} thawed");
    assert!(
        thawed >= 2,
        "thawed, the view has a hardware buffer to draw into beside the one on screen, not {thawed}"
    );
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
}

#[test]
fn app_suspending_with_triangle_drawing_resolves_with_no_gpu_work_in_flight() {
    let (engine, shell, arrivals, time) = freezing_engine();
    let (running, _view, shell_view) = run_triangle(&engine, &shell, &arrivals, |_| {});
    shell_view.wait_for_presents(30);

    block_on(engine.app_suspending());
    let presented = shell_view.presents();
    time.elapse(AFTER_HIDDEN);
    wait_until_idle(&running);
    assert_eq!(
        shell_view.presents(),
        presented,
        "no frame reaches the shell after the app suspended: none was in flight"
    );

    engine.app_resumed();
    shell_view.wait_for_presents(presented + 30);
    running.stop();
    assert!(matches!(block_on(running.ended()), ProgramEnd::Stopped));
}
