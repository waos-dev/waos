//! `examples/waft-image`, a Waft app over `wasdk-adapter-waft` that draws a PNG of its own bytes
//! with Waft's `Image::memory`, under the test shell: the bytes reach the shell's image codecs, its
//! picture decodes there at its full size, by the hardware codecs where the shell has them and into
//! the program's memory where it has none, and the window's picture is the colour the codecs
//! decoded. It is a debug build of Waft, slow to start, and draws, so it runs where the engine
//! makes hardware buffers, on Apple's systems.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use waos::{ProgramEnd, ProgramEnvironment, ProgramOutput, ProgramView, RunningProgram, WaOS};

use crate::shell::image_codecs::{Asked, ImageAnswers, ImageFacts, OpenedFrom, TestImageCodecs};
use crate::shell::{self, Lines, VIEW_HEIGHT, VIEW_WIDTH, WITHIN, pixel_of, wait_until};

/// The length of the program's PNG, `examples/waft-image/src/red.png`.
const PNG_BYTES: usize = 73;

/// The colour the shell's codecs decode every pixel in, as the bytes of an RGBA pixel.
const DECODED: [u8; 4] = [40, 200, 90, 255];

/// That colour as the bytes of the window's picture lie: blue, green, red, alpha.
const DECODED_BGRA: [u8; 4] = [90, 200, 40, 255];

#[test]
fn a_memory_image_decodes_by_the_hardware_codecs_and_draws() {
    let (engine, shell, arrivals) = shell::engine();
    shell.image_codecs().answer_with(answers());
    let (running, view) = run(&engine, &arrivals);
    wait_until("the window shows the decoded colour", || {
        middle_of(&view) == Some(DECODED_BGRA)
    });
    assert_eq!(shell.image_codecs().asked(), decoded(true));
    end(&running, &view);
}

#[test]
fn a_memory_image_decodes_into_the_programs_memory_without_hardware_codecs_and_draws() {
    let codecs = Arc::new(TestImageCodecs::without_hardware());
    codecs.answer_with(answers());
    let given = Arc::clone(&codecs);
    let (engine, _shell, arrivals) = shell::engine_of(shell::config(), move |parts| {
        parts.image_codecs = Some(given);
    });
    let (running, view) = run(&engine, &arrivals);
    wait_until("the window shows the decoded colour", || {
        middle_of(&view) == Some(DECODED_BGRA)
    });
    assert_eq!(codecs.asked(), decoded(false));
    end(&running, &view);
}

/// Waft-image running, and its window's view, once it has arrived.
fn run(engine: &WaOS, arrivals: &Receiver<ProgramView>) -> (RunningProgram, ProgramView) {
    let lines = Arc::new(Lines::default());
    let program = shell::load(engine, waos_test_programs::WAFT_IMAGE);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        ..waos_local::LocalCapabilities::new().environment()
    };
    let running = engine.run(&program, environment).unwrap();
    let view = arrivals
        .recv_timeout(WITHIN)
        .expect("waft-image asks for its window");
    (running, view)
}

/// What the shell's codecs answer: a PNG of 4 by 4, its pixels `DECODED`.
fn answers() -> ImageAnswers {
    ImageAnswers {
        decoded_formats: vec!["image/png".to_owned()],
        opened: Ok(ImageFacts::still("image/png", 4.0, 4.0)),
        pixels: Ok(DECODED),
        ..ImageAnswers::default()
    }
}

/// The program's PNG opened from its bytes, and its one frame decoded at its full size, by the
/// hardware codecs where `hardware`.
fn decoded(hardware: bool) -> Vec<Asked> {
    vec![
        Asked::Open {
            program: "waft-image".to_owned(),
            from: OpenedFrom::Bytes(PNG_BYTES),
        },
        Asked::Decode {
            index: 0,
            max_size: None,
            hardware,
        },
    ]
}

/// The middle pixel of what `view` shows, once it shows a picture of the shell's view size.
fn middle_of(view: &ProgramView) -> Option<[u8; 4]> {
    let picture = view.picture().expect("the screenshot is drawn")?;
    assert_eq!((picture.width, picture.height), (VIEW_WIDTH, VIEW_HEIGHT));
    Some(pixel_of(
        &picture,
        VIEW_WIDTH as usize / 2,
        VIEW_HEIGHT as usize / 2,
    ))
}

/// Closes the window, which ends the program.
fn end(running: &RunningProgram, view: &ProgramView) {
    view.close_requested();
    let end = shell::wait_for_end(running);
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
}
