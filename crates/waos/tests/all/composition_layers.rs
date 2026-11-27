//! A program's composition layers under the test shell: `examples/composition-layers` makes an
//! opaque layer and one that is not on the shell's compositor, sets its view's layers to both and
//! presents its first frame, then to the second alone, moved, and presents its second. Once both
//! frames are presented, the headless view holds the second frame's layer with its placement and
//! transform (the video is no longer passed), and once the program has ended, the engine holds
//! none of the layers it made. A screenshot of its view shows a video's frame on its badge whole
//! at the frame's shape, centred, where the badge's placement puts it, and nothing of a layer its
//! last picture no longer holds (the design's decision 62). The compositor shows a program's
//! buffers in the formats the shell's compositor takes that the engine makes. Under a shell with
//! no compositor, the
//! program's `get-compositor` answers none, and its presents carry no layers.
//!
//! Presents carry hardware buffers, which are made on Apple's systems, where these tests run;
//! elsewhere one test in their place, in `main.rs`, says so and passes.

use std::path::Path;
use std::sync::Arc;

use waos::bindings::wasdk::platform::hardware_buffers::HardwareBufferFormat;
use waos::bindings::wasdk::platform::views::{Mutator, Placement};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};
use waos::{ProgramEnvironment, ShellCompositor, WaOS};

use crate::test_shell::composition::{self, PresentedLayer};
use crate::test_shell::video_frames::{HeldFrame, frame_sized};
use crate::test_shell::{
    RecordedOutput, TestShell, config, pixel_of, printing_to, wait_until, within,
};

/// The program's first layer, made opaque, and its second.
const VIDEO: usize = 0;
const BADGE: usize = 1;

#[test]
fn a_present_hands_the_shell_the_layers_the_view_holds_in_order_with_their_placements() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(
            Path::new(waos_test_programs::COMPOSITION_LAYERS),
            &waos_test_programs::code_of(waos_test_programs::COMPOSITION_LAYERS),
        )
        .unwrap();
    let running = engine
        .run(&program, ProgramEnvironment::default())
        .expect("the program runs");
    let shown = within(shell.headless.view(0)).expect("its view arrives");
    wait_until("two frames presented", || shown.pictures_shown() >= 2);

    let made = shell.compositor.layers_made();
    let opaque: Vec<(usize, bool)> = made.iter().map(|l| (l.number, l.opaque)).collect();
    assert_eq!(opaque, [(VIDEO, true), (BADGE, false)]);
    assert_eq!(
        described(&composition::presented(shown.layers())),
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
    within(running.ended());
    // One for the compositor's list, one for `made`.
    wait_until("the engine holds no layer of the ended program", || {
        made.iter().all(|layer| Arc::strong_count(layer) <= 2)
    });
}

#[test]
fn a_program_hears_the_formats_the_shells_compositor_takes_that_the_engine_makes() {
    let shell = TestShell::new();
    let taken: Vec<&str> = shell
        .compositor
        .accepted_buffers()
        .gpu_memory
        .iter()
        .flat_map(|memory| memory.formats.iter())
        .map(|format| match format.format {
            HardwareBufferFormat::Bgra8 => "bgra8",
            HardwareBufferFormat::Rgba8 => "rgba8",
            HardwareBufferFormat::Nv12 => "nv12",
            HardwareBufferFormat::P010 => "p010",
        })
        .collect();
    assert_eq!(
        taken,
        ["bgra8"],
        "the test shell's compositor takes BGRA, which Apple's systems make"
    );
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(
            Path::new(waos_test_programs::COMPOSITION_LAYERS),
            &waos_test_programs::code_of(waos_test_programs::COMPOSITION_LAYERS),
        )
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine
        .run(&program, printing_to(&output))
        .expect("the program runs");
    output.wait_for(&format!("layers made, shown in {}", taken.join(", ")));
    running.stop();
    within(running.ended());
}

/// A video's frame, as its bytes lie: orange.
const ORANGE: [u8; 4] = [26, 128, 230, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

#[test]
fn a_screenshot_shows_a_frame_on_a_layer_whole_where_the_last_picture_places_the_layer() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(
            Path::new(waos_test_programs::COMPOSITION_LAYERS),
            &waos_test_programs::code_of(waos_test_programs::COMPOSITION_LAYERS),
        )
        .unwrap();
    let running = engine
        .run(&program, ProgramEnvironment::default())
        .expect("the program runs");
    let shown = within(shell.headless.view(0)).expect("its view arrives");
    let view = shown.view().expect("its view");
    wait_until("two frames presented", || shown.pictures_shown() >= 2);
    let made = shell.compositor.layers_made();
    // Square, where the badge is twice as wide as it is high.
    made[BADGE].show_frame(HeldFrame::new(frame_sized(48, 48, ORANGE)));
    made[VIDEO].show_frame(HeldFrame::new(frame_sized(64, 48, ORANGE)));

    // The badge lies at 32, 32, 96 by 48, scaled by 2: from 64, 64 to 256, 160; its frame from
    // 112 to 208 across.
    let screenshot = view.picture().unwrap().expect("a picture");
    assert_eq!(pixel_of(&screenshot, 160, 112), ORANGE);
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
    let screenshot = view.picture().unwrap().expect("a picture");
    assert_eq!(pixel_of(&screenshot, 118, 112), CLEAR);
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
    within(running.ended());
}

#[test]
fn under_a_shell_without_a_compositor_a_program_gets_none_and_presents_no_layers() {
    let shell = TestShell::new();
    let mut parts = shell.shell();
    parts.compositor = None;
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let program = engine
        .load(
            Path::new(waos_test_programs::COMPOSITION_LAYERS),
            &waos_test_programs::code_of(waos_test_programs::COMPOSITION_LAYERS),
        )
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine
        .run(&program, printing_to(&output))
        .expect("the program runs");
    let shown = within(shell.headless.view(0)).expect("its view arrives");
    let view = shown.view().expect("its view");
    output.wait_for("no compositor");
    assert!(within(view.picture_after_events()), "its first picture");
    assert!(shell.compositor.layers_made().is_empty());
    assert!(shown.layers().is_empty());
    running.stop();
    within(running.ended());
}

/// `layers` as text: the generated records have no `PartialEq` (the bindings' docs), so they
/// are compared by what `Debug` writes, every field of every mutator.
fn described(layers: &[PresentedLayer]) -> String {
    format!("{layers:#?}")
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
