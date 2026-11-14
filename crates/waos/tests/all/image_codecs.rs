//! `examples/image-codecs` under the test shell: an image opens from a file of the program's at the
//! file's path on the machine, or from bytes the program read, and what the shell's codecs say of
//! it reaches the program, its format, size, frames, orientation and metadata, its location among
//! it; its frame decodes into the program's memory, fitting the size asked, and into a hardware
//! buffer whose pixels the program reads back, as the engine reads a pixel buffer on an
//! `IOSurface`; and both encode again, the hardware buffer reaching the shell as the system's own
//! with its pixels. A path outside the program's files is not found and reaches no shell; a shell
//! without hardware codecs gives the program none, and one without codecs gives it none at all. Its
//! hardware buffers are Apple's systems', where the engine takes them.

use std::fs;
use std::sync::Arc;

use tempfile::TempDir;
use waos::bindings::wasdk::media::image_codecs::{
    DateTime, ImageMetadata, ImageOrientation, ImageRotation, Location,
};
use waos::{ProgramEnd, ProgramEnvironment, ProgramOutput, RunningProgram, WaOS};
use waos_capabilities::ProgramFolders;

use crate::shell;
use crate::shell::Lines;
use crate::shell::image_codecs::{Asked, ImageAnswers, ImageFacts, OpenedFrom, TestImageCodecs};

/// What the program's photo file holds: not an image, which the shell's codecs never read.
const PHOTO: &str = "a photo's bytes";

#[test]
fn an_image_says_what_it_is_and_decodes_into_both_memories_and_encodes_again() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.image_codecs().answer_with(answers());
    let run = Run::start(&engine, "/data/photo.jpg\nbytes /data/photo.jpg\n");
    let photo = "/data/photo.jpg";
    for line in [
        "formats: decodes image/jpeg, image/png; encodes image/png, image/jpeg".to_owned(),
        format!("{photo}: image/jpeg 32x48, frames: still, turned quarter-turn"),
        format!("{photo}: taken 2024-05-01 10:20:30 +02:00"),
        format!("{photo}: camera Waft Test Camera, lens Test Lens"),
        format!(
            "{photo}: exposure 1/120 s, f/1.8, ISO 200, 4.2 mm (26 mm equivalent), -0.33 EV, flash fired"
        ),
        format!("{photo}: location 48.85083, -2.29500, 35 m"),
        format!("{photo}: pixels 11x16 rgba8 premultiplied, all red"),
        format!("{photo}: encoded image/png opens 32x48"),
        format!(
            "{photo}: hardware buffer 32x48 for gpu-sampled, composer-overlay, cpu-read, read 32x48 bgra8 premultiplied, all red"
        ),
        format!("{photo}: hardware encoded image/jpeg opens 32x48"),
        format!("bytes {photo}: image/jpeg 32x48, frames: still, turned quarter-turn"),
    ] {
        run.lines.wait_for(&line);
    }
    assert!(matches!(
        shell::wait_for_end(&run.running),
        ProgramEnd::Returned(Ok(()))
    ));

    let asked = shell.image_codecs().asked();
    let file = fs::canonicalize(run.data()).unwrap().join("photo.jpg");
    let opened_from = |from| Asked::Open {
        program: "image-codecs".to_owned(),
        from,
    };
    assert_eq!(
        asked[..7],
        [
            opened_from(OpenedFrom::File(file)),
            Asked::Decode {
                frame: 0,
                max_size: Some((16.0, 16.0)),
                hardware: false,
            },
            Asked::Encode {
                width: 11,
                height: 16,
                format: "image/png".to_owned(),
                quality: None,
                hardware_first_pixel: None,
            },
            opened_from(OpenedFrom::Bytes(b"encoded".len())),
            Asked::Decode {
                frame: 0,
                max_size: None,
                hardware: true,
            },
            Asked::Encode {
                width: 32,
                height: 48,
                format: "image/jpeg".to_owned(),
                quality: Some(0.8),
                hardware_first_pixel: Some([0, 0, 255, 255]),
            },
            opened_from(OpenedFrom::Bytes(b"encoded".len())),
        ],
        "the program's hardware buffer reaches the shell as the system's own, red in BGRA"
    );
    assert_eq!(asked[7], opened_from(OpenedFrom::Bytes(PHOTO.len())));
}

#[test]
fn a_path_outside_the_programs_files_is_not_found_and_reaches_no_shell() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.image_codecs().answer_with(answers());
    let run = Run::start(&engine, "/elsewhere/photo.jpg\n");
    run.lines.wait_for(
        "/elsewhere/photo.jpg: open failed (not-found: no file at /elsewhere/photo.jpg among the program's)",
    );
    shell::wait_for_end(&run.running);
    assert_eq!(
        shell.image_codecs().asked(),
        [],
        "nothing reaches the shell"
    );
}

#[test]
fn a_shell_without_hardware_codecs_gives_the_program_none() {
    let codecs = Arc::new(TestImageCodecs::without_hardware());
    codecs.answer_with(answers());
    let given = Arc::clone(&codecs);
    let (engine, _shell, _arrivals) = shell::engine_of(shell::config(), move |parts| {
        parts.image_codecs = Some(given);
    });
    let run = Run::start(&engine, "/data/photo.jpg\n");
    run.lines.wait_for("hardware codecs: none");
    run.lines
        .wait_for("/data/photo.jpg: encoded image/png opens 32x48");
    shell::wait_for_end(&run.running);
    assert!(
        !run.lines
            .all()
            .iter()
            .any(|line| line.contains("hardware buffer")),
        "{:?}",
        run.lines.all()
    );
    assert!(
        codecs
            .asked()
            .iter()
            .all(|asked| !matches!(asked, Asked::Decode { hardware: true, .. })),
        "no hardware decoding asked"
    );
}

#[test]
fn a_shell_without_image_codecs_gives_the_program_none() {
    let (engine, _shell, _arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.image_codecs = None;
    });
    let run = Run::start(&engine, "/data/photo.jpg\n");
    assert!(matches!(
        shell::wait_for_end(&run.running),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(run.lines.all(), ["image codecs: none"]);
}

/// What the shell's codecs answer: a JPEG photo of 32 by 48 upright, stored turned a quarter, with
/// EXIF's common fields and a location, its pixels red, and every encoding `encoded`.
fn answers() -> ImageAnswers {
    ImageAnswers {
        decoded_formats: vec!["image/jpeg".to_owned(), "image/png".to_owned()],
        encoded_formats: vec!["image/png".to_owned(), "image/jpeg".to_owned()],
        opened: Ok(ImageFacts {
            orientation: ImageOrientation {
                mirrored: false,
                rotation: ImageRotation::QuarterTurn,
            },
            metadata: ImageMetadata {
                taken: Some(DateTime {
                    year: 2024,
                    month: 5,
                    day: 1,
                    hour: 10,
                    minute: 20,
                    second: 30,
                    utc_offset: Some(120),
                }),
                camera_make: Some("Waft".to_owned()),
                camera_model: Some("Test Camera".to_owned()),
                lens_model: Some("Test Lens".to_owned()),
                exposure_time: Some(1.0 / 120.0),
                f_number: Some(1.8),
                iso: Some(200),
                focal_length: Some(4.2),
                equivalent_focal_length: Some(26.0),
                exposure_bias: Some(-1.0 / 3.0),
                flash_fired: Some(true),
                location: Some(Location {
                    latitude: 48.850833,
                    longitude: -2.295,
                    altitude: Some(35.0),
                }),
            },
            ..ImageFacts::still("image/jpeg", 32.0, 48.0)
        }),
        pixels: Ok([255, 0, 0, 255]),
        encoded: Ok(b"encoded".to_vec()),
    }
}

/// `examples/image-codecs` running, the images it opens written in its `/data/sources`, beside a
/// photo of its own, `/data/photo.jpg`.
struct Run {
    running: RunningProgram,
    lines: Arc<Lines>,
    folders: TempDir,
}

impl Run {
    fn start(engine: &WaOS, sources: &str) -> Run {
        let folders = TempDir::new().unwrap();
        let own = ProgramFolders {
            data: folders.path().join("data"),
            cache: folders.path().join("cache"),
        };
        fs::create_dir_all(&own.data).unwrap();
        fs::create_dir_all(&own.cache).unwrap();
        fs::write(own.data.join("sources"), sources).unwrap();
        fs::write(own.data.join("photo.jpg"), PHOTO).unwrap();
        let lines = Arc::new(Lines::default());
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
            folders: Some(own),
            ..ProgramEnvironment::default()
        };
        let program = shell::load(engine, waos_test_programs::IMAGE_CODECS);
        let (running, _given) = engine.run(&program, environment, Vec::new()).unwrap();
        Run {
            running,
            lines,
            folders,
        }
    }

    /// The program's own `/data`, on the machine.
    fn data(&self) -> std::path::PathBuf {
        self.folders.path().join("data")
    }
}
