//! `examples/video-frames` under the test shell: the program makes a reader on the shell's
//! readers and gives it to a player, which the shell's player is given as its own reader; at each
//! frame of its view the program asks the reader for the frame of that frame's display time, which
//! the shell's reader is asked for as the moment the shell said the refresh's picture reaches the
//! screen; and the frame the shell hands out reaches the program, which reads its first pixel back
//! through a texture it imports of its hardware buffer on its own device (`gpu-interop`), the
//! frame's own pixels with no copy. A shell whose system hands out no frames gives the program no readers.

use std::fs;
use std::sync::Arc;

use tempfile::TempDir;
use waos::{ProgramEnd, ProgramEnvironment, ProgramOutput, RunningProgram, WaOS};
use waos_capabilities::ProgramFolders;

use crate::shell::media_players::{Destination, PlayerCall};
use crate::shell::video_frames::{FRAME_HEIGHT, FRAME_WIDTH};
use crate::shell::{self, Lines, wait_until};

#[cfg(target_vendor = "apple")]
#[test]
fn a_readers_frame_for_a_frames_display_time_reaches_the_program_and_its_device() {
    let (engine, shell, _arrivals) = shell::engine();
    let run = Run::start(&engine);
    run.lines.wait_for("frames to the reader");
    let reader = &shell.video_frame_readers().made()[0];
    let player = &shell.media_players().opened()[0];
    assert_eq!(
        player.calls(),
        [
            PlayerCall::SetVideoOutput(Some(Destination::Frames(reader.number))),
            PlayerCall::Play
        ],
        "the player sends its frames to the reader the shell made"
    );

    wait_until("the program asks its reader for a frame", || {
        !reader.asked().is_empty()
    });
    reader.put_frame([0, 0, 255, 255]);
    let said = format!("frame {FRAME_WIDTH}x{FRAME_HEIGHT}, first pixel [0, 0, 255, 255]");
    run.lines.wait_for(&said);
    let display_times = shell.display_times();
    for asked in reader.asked() {
        assert!(
            display_times.contains(&asked),
            "asked for {asked:?}, which no refresh's display time is"
        );
    }
    assert!(matches!(
        shell::wait_for_end(&run.running),
        ProgramEnd::Returned(Ok(()))
    ));
}

#[test]
fn a_shell_whose_system_hands_out_no_frames_gives_the_program_no_readers() {
    let (engine, _shell, _arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.video_frame_readers = None;
    });
    let run = Run::start(&engine);
    assert!(matches!(
        shell::wait_for_end(&run.running),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(run.lines.all(), ["video frame readers: none"]);
}

/// `examples/video-frames` running, a clip of its own at `/data/clip.mp4`.
struct Run {
    running: RunningProgram,
    lines: Arc<Lines>,
    _folders: TempDir,
}

impl Run {
    fn start(engine: &WaOS) -> Run {
        let folders = TempDir::new().unwrap();
        let own = ProgramFolders {
            data: folders.path().join("data"),
            cache: folders.path().join("cache"),
        };
        fs::create_dir_all(&own.data).unwrap();
        fs::create_dir_all(&own.cache).unwrap();
        fs::write(own.data.join("clip.mp4"), "a clip").unwrap();
        let lines = Arc::new(Lines::default());
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
            folders: Some(own),
            ..ProgramEnvironment::default()
        };
        let program = shell::load(engine, waos_test_programs::VIDEO_FRAMES);
        let running = engine.run(&program, environment).unwrap();
        Run {
            running,
            lines,
            _folders: folders,
        }
    }
}
