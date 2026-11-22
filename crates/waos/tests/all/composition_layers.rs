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

use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};

use waos::bindings::wasdk::platform::views::{
    Cursor, Mutator, OverlayStyle, Placement, RoundedRect, ViewError, ViewFocusDirection,
    ViewFocusState,
};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};
use waos::{
    HardwareBuffer, PlacedView, Presence, ProgramEnvironment, ProgramView, RunningProgram,
    ShellCompositionLayer, ShellView, ViewPlacement, ViewRequest, WaOS,
};
use waos_headless::SCREEN;

use crate::test_shell::composition::{self, PresentedLayer};
use crate::test_shell::video_frames::{HeldFrame, frame_sized};
use crate::test_shell::{
    DISPLAY, RecordedOutput, TestShell, WITHIN, config, metrics, pixel_of, printing_to, wait_until,
    within,
};

/// The program's first layer, made opaque, and its second.
const VIDEO: usize = 0;
const BADGE: usize = 1;

#[test]
fn a_present_hands_the_shell_the_layers_the_view_holds_in_order_with_their_placements() {
    let shell = TestShell::new();
    let recorded = Arc::new(RecordingLayersView::default());
    let (arrivals, arrived) = mpsc::channel();
    let mut parts = shell.shell();
    parts.views = Arc::new(RecordingPlacement {
        view: Arc::clone(&recorded),
        arrivals,
    });
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::COMPOSITION_LAYERS))
        .unwrap();
    let running = engine
        .run(&program, ProgramEnvironment::default())
        .expect("the program runs");
    let _view = arrived.recv_timeout(WITHIN).expect("its view arrives");
    wait_until("two frames presented", || {
        recorded.presented_layers().len() >= 2
    });

    let made = shell.compositor.layers_made();
    let opaque: Vec<(usize, bool)> = made.iter().map(|l| (l.number, l.opaque)).collect();
    assert_eq!(opaque, [(VIDEO, true), (BADGE, false)]);
    let presents = recorded.presented_layers();
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
    within(running.ended());
    // One for the compositor's list, one for `made`.
    wait_until("the engine holds no layer of the ended program", || {
        made.iter().all(|layer| Arc::strong_count(layer) <= 2)
    });
}

/// A video's frame, as its bytes lie: orange.
const ORANGE: [u8; 4] = [26, 128, 230, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

#[test]
fn a_screenshot_shows_a_frame_on_a_layer_whole_where_the_last_picture_places_the_layer() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::COMPOSITION_LAYERS))
        .unwrap();
    let running = engine
        .run(&program, ProgramEnvironment::default())
        .expect("the program runs");
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("its view arrives");
    wait_until("two frames presented", || {
        running.stats().frames_presented() >= 2
    });
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
    let recorded = Arc::new(RecordingLayersView::default());
    let (arrivals, arrived) = mpsc::channel();
    let mut parts = shell.shell();
    parts.compositor = None;
    parts.views = Arc::new(RecordingPlacement {
        view: Arc::clone(&recorded),
        arrivals,
    });
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::COMPOSITION_LAYERS))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine
        .run(&program, printing_to(&output))
        .expect("the program runs");
    let _view = arrived.recv_timeout(WITHIN).expect("its view arrives");
    wait_until("two frames presented", || {
        recorded.presented_layers().len() >= 2
    });
    output.wait_for("no compositor");
    assert!(shell.compositor.layers_made().is_empty());
    assert!(
        recorded.presented_layers().iter().all(Vec::is_empty),
        "{:#?}",
        recorded.presented_layers()
    );
    running.stop();
    within(running.ended());
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

/// A view that records which composition layers each present named.
#[derive(Default)]
struct RecordingLayersView {
    presented_layers: Mutex<Vec<Vec<PresentedLayer>>>,
    shown: Mutex<Option<Box<dyn FnOnce() + Send + Sync>>>,
}

impl RecordingLayersView {
    fn presented_layers(&self) -> Vec<Vec<PresentedLayer>> {
        self.presented_layers.lock().unwrap().clone()
    }
}

impl ShellView for RecordingLayersView {
    fn present(
        &self,
        _buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    ) {
        self.presented_layers
            .lock()
            .unwrap()
            .push(composition::presented(layers));
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

/// Places a [`RecordingLayersView`] on the test shell's screen and hands the [`ProgramView`] back.
struct RecordingPlacement {
    view: Arc<RecordingLayersView>,
    arrivals: mpsc::Sender<ProgramView>,
}

impl ViewPlacement for RecordingPlacement {
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
