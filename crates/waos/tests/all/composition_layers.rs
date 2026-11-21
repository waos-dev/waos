//! A program's composition layers under the test shell: `examples/composition-layers` makes an
//! opaque layer and one that is not on the shell's compositor, sets its view's layers to both and
//! presents its first frame, then to the second alone, moved, and presents its second. The shell's
//! present gets the layers the view held as it presented, in the order set, each the layer the
//! shell's compositor made for it, with its placement as the program gave it, every kind of
//! mutator carried; a layer the view no longer holds is not passed with a later present; and once
//! the program has ended, the engine holds none of the layers it made. A screenshot of its view
//! shows a video's frame on its badge whole at the frame's shape, centred, where the badge's
//! placement puts it, and nothing of a layer its last picture no longer holds (the design's
//! decision 62). Under a shell with no compositor, the program's `get-compositor` answers none,
//! and its presents carry no layers.
//!
//! Presents carry hardware buffers, which are made on Apple's systems, where these tests run;
//! elsewhere one test in their place, in `main.rs`, says so and passes.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use waos::bindings::wasdk::platform::views::{Mutator, Placement, RoundedRect};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};
use waos::{ProgramEnvironment, ProgramOutput};

use crate::shell::composition::PresentedLayer;
use crate::shell::video_frames::{HeldFrame, frame_sized};
use crate::shell::{self, Lines, WITHIN, pixel_of, screenshot_of};

/// The program's first layer, made opaque, and its second.
const VIDEO: usize = 0;
const BADGE: usize = 1;

#[test]
fn a_present_hands_the_shell_the_layers_the_view_holds_in_order_with_their_placements() {
    let (engine, shell, arrivals) = shell::engine();
    let program = shell::load(&engine, waos_test_programs::COMPOSITION_LAYERS);
    let running = engine
        .run(&program, ProgramEnvironment::default())
        .expect("the program runs");
    let _view = arrivals.recv_timeout(WITHIN).expect("its view arrives");
    let placed = shell.placed_views()[0].clone();
    placed.wait_for_presents(2);

    let made = shell.compositor().layers_made();
    let opaque: Vec<(usize, bool)> = made.iter().map(|l| (l.number, l.opaque)).collect();
    assert_eq!(opaque, [(VIDEO, true), (BADGE, false)]);
    let presents = placed.presented_layers();
    assert_eq!(
        described(&presents[0]),
        described(&[
            PresentedLayer {
                number: VIDEO,
                placement: video_placement(),
            },
            PresentedLayer {
                number: BADGE,
                placement: Placement {
                    rect: rect(16.0, 16.0, 96.0, 48.0),
                    mutators: Vec::new(),
                },
            },
        ]),
        "the first frame shows both, the video beneath"
    );
    assert_eq!(
        described(&presents[1]),
        described(&[PresentedLayer {
            number: BADGE,
            placement: Placement {
                rect: rect(32.0, 32.0, 96.0, 48.0),
                mutators: vec![Mutator::Transform(scale(2.0))],
            },
        }]),
        "the second frame shows the badge alone, so the video is not passed"
    );
    assert!(
        made.iter().all(|layer| Arc::strong_count(layer) > 2),
        "the engine holds each layer while the program does"
    );
    running.stop();
    block_on(running.ended());
    wait_until_held_by_the_compositor_alone(&made);
}

/// A video's frame, as its bytes lie: orange.
const ORANGE: [u8; 4] = [26, 128, 230, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

#[test]
fn a_screenshot_shows_a_frame_on_a_layer_whole_where_the_last_picture_places_the_layer() {
    let (engine, shell, arrivals) = shell::engine();
    let program = shell::load(&engine, waos_test_programs::COMPOSITION_LAYERS);
    let running = engine
        .run(&program, ProgramEnvironment::default())
        .expect("the program runs");
    let view = arrivals.recv_timeout(WITHIN).expect("its view arrives");
    shell.placed_views()[0].wait_for_presents(2);
    let made = shell.compositor().layers_made();
    // Square, where the badge is twice as wide as it is high.
    made[BADGE].show_frame(HeldFrame::new(frame_sized(48, 48, ORANGE)));
    made[VIDEO].show_frame(HeldFrame::new(frame_sized(64, 48, ORANGE)));

    // The badge lies at 32, 32, 96 by 48, scaled by 2: from 64, 64 to 256, 160; its frame from
    // 112 to 208 across.
    let deadline = Instant::now() + WITHIN;
    let screenshot = loop {
        let screenshot = screenshot_of(&view);
        if pixel_of(&screenshot, 160, 112) == ORANGE {
            break screenshot;
        }
        assert!(
            Instant::now() < deadline,
            "the badge never showed its frame"
        );
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(pixel_of(&screenshot, 113, 65), ORANGE);
    assert_eq!(pixel_of(&screenshot, 206, 158), ORANGE);
    assert_eq!(
        pixel_of(&screenshot, 80, 112),
        CLEAR,
        "beside the frame, kept at its shape"
    );
    assert_eq!(pixel_of(&screenshot, 240, 112), CLEAR);
    assert_eq!(
        pixel_of(&screenshot, 40, 40),
        CLEAR,
        "the video, which the last picture no longer holds"
    );

    // A 64 by 48 frame shown turned a quarter, as a phone's camera frame is: whole at 48 by 64,
    // so 72 by 96 of the badge's 192 by 96, from 124 to 196 across.
    let turned = waos::VideoFrame {
        rotation: 90,
        ..frame_sized(64, 48, ORANGE)
    };
    made[BADGE].show_frame(HeldFrame::new(turned));
    let deadline = Instant::now() + WITHIN;
    let screenshot = loop {
        let screenshot = screenshot_of(&view);
        if pixel_of(&screenshot, 118, 112) == CLEAR {
            break screenshot;
        }
        assert!(
            Instant::now() < deadline,
            "the badge never showed its frame turned"
        );
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(pixel_of(&screenshot, 160, 112), ORANGE);
    assert_eq!(
        pixel_of(&screenshot, 126, 66),
        ORANGE,
        "the turned frame's top-left"
    );
    assert_eq!(
        pixel_of(&screenshot, 202, 112),
        CLEAR,
        "beside it, at its turned shape"
    );
    running.stop();
    block_on(running.ended());
}

/// Waits until the test compositor's list is all that holds each of `layers`, failing after
/// [`WITHIN`]: the engine holds a layer for as long as its program or one of its views does,
/// and a present only until it reaches the shell.
fn wait_until_held_by_the_compositor_alone(layers: &[Arc<shell::composition::TestLayer>]) {
    let deadline = Instant::now() + WITHIN;
    // One for the compositor's list, one for `layers`.
    while layers.iter().any(|layer| Arc::strong_count(layer) > 2) {
        assert!(
            Instant::now() < deadline,
            "the engine still holds a layer of the ended program"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn under_a_shell_without_a_compositor_a_program_gets_none_and_presents_no_layers() {
    let (engine, shell, arrivals) =
        shell::engine_of(shell::config(), |parts| parts.compositor = None);
    let program = shell::load(&engine, waos_test_programs::COMPOSITION_LAYERS);
    let output = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&output) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let running = engine.run(&program, environment).expect("the program runs");
    let _view = arrivals.recv_timeout(WITHIN).expect("its view arrives");
    let placed = shell.placed_views()[0].clone();
    placed.wait_for_presents(2);
    output.wait_for("no compositor");
    assert!(shell.compositor().layers_made().is_empty());
    assert!(
        placed.presented_layers().iter().all(Vec::is_empty),
        "{:#?}",
        placed.presented_layers()
    );
    running.stop();
}

/// `layers` as text: the generated records have no `PartialEq` (the bindings' docs), so they
/// are compared by what `Debug` writes, every field of every mutator.
fn described(layers: &[PresentedLayer]) -> String {
    format!("{layers:#?}")
}

/// The placement the program gives its video in its first frame.
fn video_placement() -> Placement {
    let corner = Size {
        width: 8.0,
        height: 8.0,
    };
    Placement {
        rect: rect(0.0, 0.0, 640.0, 360.0),
        mutators: vec![
            Mutator::Transform(scale(2.0)),
            Mutator::ClipRect(rect(0.0, 0.0, 600.0, 300.0)),
            Mutator::ClipRoundedRect(RoundedRect {
                rect: rect(10.0, 10.0, 300.0, 200.0),
                top_left: corner,
                top_right: corner,
                bottom_right: corner,
                bottom_left: corner,
            }),
            Mutator::Opacity(128),
        ],
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        origin: Point { x, y },
        size: Size { width, height },
    }
}

/// A scale by `factor` on both axes, column-major.
fn scale(factor: f32) -> Vec<f32> {
    vec![
        factor, 0.0, 0.0, 0.0, //
        0.0, factor, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]
}
