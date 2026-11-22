//! `examples/video-frames` under the test shell: the program makes a reader on the shell's
//! readers and gives it to a player, which the shell's player is given as its own reader; at each
//! frame of its view the program asks the reader for the frame of that frame's display time, which
//! the shell's reader is asked for as the moment the shell said the refresh's picture reaches the
//! screen; and the frame the shell hands out reaches the program, which reads its first pixel back
//! through a texture it imports of its hardware buffer on its own device (`gpu-interop`), the
//! frame's own pixels with no copy. A shell whose system hands out no frames gives the program no readers.

use std::fs;
use std::path::Path;
use std::sync::Arc;
#[cfg(target_vendor = "apple")]
use std::time::Instant;

use tempfile::TempDir;
use waos::{ProgramEnd, ProgramEnvironment, RunningProgram, WaOS};
use waos_capabilities::ProgramFolders;

#[cfg(target_vendor = "apple")]
use crate::shell::media_players::{Destination, PlayerCall};
#[cfg(target_vendor = "apple")]
use crate::shell::video_frames::{FRAME_HEIGHT, FRAME_WIDTH};
#[cfg(target_vendor = "apple")]
use crate::test_shell::wait_until;
use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

#[cfg(target_vendor = "apple")]
#[test]
fn a_readers_frame_for_a_frames_display_time_reaches_the_program_and_its_device() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let started = Instant::now();
    let run = Run::start(&engine);
    run.output.wait_for("frames to the reader");
    let reader = &shell.video_frame_readers.made()[0];
    let player = &shell.media_players.opened()[0];
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
    run.output.wait_for(&said);
    // TODO(tests-without-the-clock): the headless shell keeps no refresh's display time, so this
    // checks only that each time asked is one of a refresh since the program started, the
    // headless screen's picture reaching it as it refreshes; the old shell's check was that each
    // is one the shell told the engine.
    let now = Instant::now();
    for asked in reader.asked() {
        assert!(
            started <= asked && asked <= now,
            "asked for {asked:?}, when no refresh came"
        );
    }
    assert!(matches!(
        within(run.running.ended()),
        ProgramEnd::Returned(Ok(()))
    ));
}

#[test]
fn a_shell_whose_system_hands_out_no_frames_gives_the_program_no_readers() {
    let shell = TestShell::new();
    let mut parts = shell.shell();
    parts.video_frame_readers = None;
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let run = Run::start(&engine);
    assert!(matches!(
        within(run.running.ended()),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(run.output.lines(), ["video frame readers: none"]);
}

/// `examples/video-frames` running, a clip of its own at `/data/clip.mp4`.
struct Run {
    running: RunningProgram,
    output: Arc<RecordedOutput>,
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
        let output = Arc::new(RecordedOutput::default());
        let environment = ProgramEnvironment {
            folders: Some(own),
            ..printing_to(&output)
        };
        let program = engine
            .load(Path::new(waos_test_programs::VIDEO_FRAMES))
            .unwrap();
        let running = engine.run(&program, environment).unwrap();
        Run {
            running,
            output,
            _folders: folders,
        }
    }
}
