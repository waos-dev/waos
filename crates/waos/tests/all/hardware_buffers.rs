//! A program's own hardware buffers under the test shell: `examples/hardware-buffers` allocates two
//! buffers with the system's allocator, after the engine refuses RGBA and a side past the largest texture,
//! the red one, asked for no reading back, answering every use, as any buffer a program allocates
//! on Apple's systems does;
//! through textures it imports of them on its own device, clears one red with a render pass and writes the
//! other green with `write_texture` alone, which no submission of its own carries; reads the green
//! one back, seeing the write, which the engine submitted and waited for, and encodes the red one
//! through the hardware codecs, seeing what the GPU drew; and presents the red one on a composition
//! layer it places beside its canvas, then the green one at each frame until the red one is back.
//! The shell's layer is handed the buffers in order, each with what the GPU drew into it, the
//! encoding is handed the red buffer, the view's present names the layer where the program placed
//! it, its canvas clear there, written after its last submission, and the program hears that the
//! red buffer is back once the shell's layer let it go at the green one's present; the bytes it
//! holds are counted in its stats while it holds them. A screenshot of its view is its canvas over
//! the green buffer its layer shows last, in the room the canvas makes for it, and the canvas as it
//! is once the layer shows nothing (the design's decision 62). Under a shell
//! with no compositor, the program draws its canvas with no layer.
//!
//! Hardware buffers are allocated on Apple's systems, where these tests run; elsewhere one test in
//! their place, in `main.rs`, says so and passes.

use std::sync::Arc;

use futures::executor::block_on;
use waos::bindings::wasdk::platform::hardware_buffers::{ImageError, ImageErrorKind};
use waos::bindings::wasdk::platform::views::{Mutator, Placement};
use waos::bindings::wasdk::platform::windows::{Point, Rect, Size};
use waos::{ProgramEnvironment, ProgramOutput, RunningProgram, WaOS};

use crate::shell::composition::PresentedLayer;
use crate::shell::hardware_buffer::{picture, pixel};
use crate::shell::image_codecs::{Asked, ImageAnswers};
use crate::shell::{self, Lines, VIEW_HEIGHT, VIEW_WIDTH, WITHIN, pixel_of, screenshot_of};

/// A pixel as the bytes lie in a BGRA buffer.
const RED: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];
/// The program's canvas where its layer does not show: a quarter of white, opaque.
const GREY: [u8; 4] = [64, 64, 64, 255];

/// The program's buffers' size, and where its layer sits.
const WIDTH: usize = 64;
const HEIGHT: usize = 48;
const LAYER_AT: usize = 16;

#[test]
fn buffers_the_program_drew_reach_the_codecs_and_the_layer_once_the_gpu_drew_them() {
    let (engine, shell, arrivals) = shell::engine();
    shell.image_codecs().answer_with(answers());
    let run = Run::start(&engine);
    let _view = arrivals.recv_timeout(WITHIN).expect("its view arrives");
    for line in [
        "rgba8: unsupported",
        "16385x48: too-large",
        "allocated 2 buffers of 64x48",
        "red can be gpu-sampled, gpu-color-output, composer-overlay, cpu-read",
        "read 64x48 bgra8 premultiplied, first pixel [0, 255, 0, 255]",
        "encoded image/png from 64x48",
        "presented red",
        "presented green",
        "red is back",
    ] {
        run.lines.wait_for(line);
    }
    let counted = run.running.stats().hardware_buffer_bytes();
    assert!(
        counted >= 2 * (WIDTH * HEIGHT * 4) as u64,
        "the program's two buffers are counted beside its view's: {counted} bytes"
    );

    let made = shell.compositor().layers_made();
    assert_eq!(made.len(), 1, "one layer");
    let presented = made[0].buffers_presented();
    assert!(presented.len() >= 2, "the red buffer, then the green one");
    assert_eq!(pixel(&presented[0], 0, 0), RED, "drawn before it was shown");
    assert_eq!(pixel(&presented[1], WIDTH - 1, HEIGHT - 1), GREEN);

    let encodings: Vec<Asked> = shell
        .image_codecs()
        .asked()
        .into_iter()
        .filter(|asked| matches!(asked, Asked::Encode { .. }))
        .collect();
    assert_eq!(
        encodings,
        [Asked::Encode {
            width: WIDTH as u32,
            height: HEIGHT as u32,
            format: "image/png".to_owned(),
            quality: None,
            hardware_first_pixel: Some(RED),
        }],
        "the red buffer reaches the codecs as the GPU drew it"
    );

    let placed = shell.placed_views()[0].clone();
    let layers = placed.presented_layers();
    assert_eq!(
        described(&layers[0]),
        described(&[PresentedLayer {
            number: 0,
            placement: layer_placement(),
        }]),
        "the view's first frame names the layer where the program placed it"
    );
    let canvas = picture(&placed.latest_hardware_buffer().expect("a frame on screen"));
    assert_eq!(
        canvas.at(LAYER_AT, LAYER_AT),
        CLEAR,
        "clear where the layer shows, written after the frame's last submission"
    );
    assert_eq!(canvas.at(LAYER_AT + WIDTH, LAYER_AT + HEIGHT), GREY);

    run.running.stop();
    block_on(run.running.ended());
}

#[test]
fn a_screenshot_is_the_canvas_over_what_its_layer_shows() {
    let (engine, shell, arrivals) = shell::engine();
    shell.image_codecs().answer_with(answers());
    let run = Run::start(&engine);
    let view = arrivals.recv_timeout(WITHIN).expect("its view arrives");
    run.lines.wait_for("red is back");
    let screenshot = screenshot_of(&view);
    assert_eq!(
        (screenshot.width, screenshot.height),
        (VIEW_WIDTH, VIEW_HEIGHT)
    );
    assert_eq!(pixel_of(&screenshot, LAYER_AT, LAYER_AT), GREEN);
    let last = (LAYER_AT + WIDTH - 1, LAYER_AT + HEIGHT - 1);
    assert_eq!(pixel_of(&screenshot, last.0, last.1), GREEN);
    assert_eq!(pixel_of(&screenshot, last.0 + 1, last.1 + 1), GREY);
    assert_eq!(pixel_of(&screenshot, 0, 0), GREY);

    shell.compositor().layers_made()[0].show_nothing();
    let screenshot = screenshot_of(&view);
    assert_eq!(
        pixel_of(&screenshot, LAYER_AT, LAYER_AT),
        CLEAR,
        "a layer that shows nothing leaves the canvas as it is"
    );
    assert_eq!(pixel_of(&screenshot, 0, 0), GREY);
    run.running.stop();
    block_on(run.running.ended());
}

#[test]
fn without_a_compositor_the_program_draws_its_canvas_with_no_layer() {
    let (engine, shell, arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.compositor = None;
    });
    shell.image_codecs().answer_with(answers());
    let run = Run::start(&engine);
    let _view = arrivals.recv_timeout(WITHIN).expect("its view arrives");
    run.lines.wait_for("no compositor");
    let placed = shell.placed_views()[0].clone();
    placed.wait_for_presents(1);
    assert!(placed.presented_layers()[0].is_empty());
    assert!(shell.compositor().layers_made().is_empty());
    run.running.stop();
    block_on(run.running.ended());
}

/// What the shell's codecs answer: PNG among the formats they encode, and every encoding a
/// few bytes.
fn answers() -> ImageAnswers {
    ImageAnswers {
        decoded_formats: vec!["image/png".to_owned()],
        encoded_formats: vec!["image/png".to_owned()],
        opened: Err(ImageError {
            kind: ImageErrorKind::Other,
            message: "this program opens no image".to_owned(),
        }),
        pixels: Ok([255, 0, 0, 255]),
        encoded: Ok(b"encoded".to_vec()),
    }
}

/// The pixel ratio of the views the tests' shell places.
const PIXEL_RATIO: f64 = 2.0;

/// Where the program places its layer: 64 by 48 physical pixels at 16, 16, as a rectangle in
/// logical pixels under a transform by the view's pixel ratio, which a layer's buffer is drawn at.
fn layer_placement() -> Placement {
    let logical = |pixels: usize| pixels as f64 / PIXEL_RATIO;
    let mut by_the_ratio = vec![0.0; 16];
    by_the_ratio[0] = PIXEL_RATIO as f32;
    by_the_ratio[5] = PIXEL_RATIO as f32;
    by_the_ratio[10] = 1.0;
    by_the_ratio[15] = 1.0;
    Placement {
        rect: Rect {
            origin: Point {
                x: logical(LAYER_AT),
                y: logical(LAYER_AT),
            },
            size: Size {
                width: logical(WIDTH),
                height: logical(HEIGHT),
            },
        },
        mutators: vec![Mutator::Transform(by_the_ratio)],
    }
}

/// Presented layers as text, which compares where the WIT's records do not.
fn described(layers: &[PresentedLayer]) -> String {
    format!("{layers:?}")
}

/// `examples/hardware-buffers` running, its output read.
struct Run {
    running: RunningProgram,
    lines: Arc<Lines>,
}

impl Run {
    fn start(engine: &WaOS) -> Run {
        let lines = Arc::new(Lines::default());
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
            ..ProgramEnvironment::default()
        };
        let program = shell::load(engine, waos_test_programs::HARDWARE_BUFFERS);
        let (running, _views) = engine.run(&program, environment, Vec::new()).unwrap();
        Run { running, lines }
    }
}
