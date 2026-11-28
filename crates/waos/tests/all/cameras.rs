//! `examples/cameras` under the test shell, with grants of the test's own that keep the answers to
//! the questions put: the cameras are listed without asking, and again each time one comes or
//! goes, and the first opens once its user
//! allows it, asked at the first use, and not at all once refused; its formats are the shell's,
//! and a session starts in the one picked with its outputs reaching the shell as the shell's own
//! layer and reader, takes a photo, tells each state the shell reports until it ends, and is shown
//! by the shell's indicator from its start until the program lets it go, which keeps the program
//! from freezing, as the app's going to the background would freeze it. A session on a camera
//! running for someone else says the format it joined; a format the camera does not list traps;
//! and a shell without cameras, or with no indicator to show a program using one, gives the
//! program none. A program whose camera answer is simulated lists and opens the shell's camera of
//! WaOS's own, asked nothing, shown recording with it apart; one whose shell has none lists none.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use futures::FutureExt;
use tempfile::TempDir;
use waos::bindings::wasdk::media::cameras::{CameraError, CameraInterruption, CaptureState};
use waos::{
    Answer, ProgramEnd, ProgramEnvironment, ProgramGrants, Question, RecordingDevice,
    RunningProgram, WaOS,
};
use waos_capabilities::{ProgramFolders, Setting};

use crate::asking::{app, permissions_with};
use crate::test_shell::cameras::{Started, formats, listed};
use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, within};

/// What the program says of the cameras listed, before it opens one.
const LISTED: [&str; 2] = [
    "camera front: FaceTime HD Camera (front)",
    "camera desk: Desk View (external)",
];

/// What the program says of the formats of the camera it opened.
const FORMATS: [&str; 2] = [
    "format 1920x1080 at 15-30 fps, photos 1920x1080 1280x720",
    "format 640x480 at 15-30 fps, photos 640x480",
];

#[test]
fn a_camera_opens_once_allowed_and_runs_on_the_shells_own_layer_and_reader_until_it_ends() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let run = Run::start(&engine, Run::asking(&shell));
    for line in LISTED {
        run.output.wait_for(line);
    }
    shell.changes.wait_until("the camera question is put", || {
        !shell.prompts.questions().is_empty()
    });
    assert_eq!(
        shell.prompts.questions(),
        [(app().to_string(), Question::Camera)],
        "listing asks nothing; opening asks once"
    );
    assert!(shell.cameras.opened().is_empty(), "nothing opens before");
    shell.prompts.answer_questions(Some(Answer::Granted));

    for line in FORMATS {
        run.output.wait_for(line);
    }
    run.output.wait_for("started in 640x480");
    assert_eq!(
        shell.cameras.opened(),
        [("cameras".to_owned(), "front".to_owned())]
    );
    let session = started(&shell);
    assert_eq!(sizes(&[session.picked.size]), ["640x480"]);
    assert_eq!(session.preview, Some(0), "the layer the program made");
    assert!(shell.compositor.layers_made()[0].opaque);
    assert_eq!(session.frames, Some(0), "the reader the program made");
    assert_eq!(shell.video_frame_readers.made().len(), 1);
    assert_eq!(
        sizes(&session.photos.into_iter().collect::<Vec<_>>()),
        ["640x480"]
    );
    run.output.wait_for("photo: image/jpeg, 3 bytes");
    run.output.wait_for("state: running");
    assert_eq!(
        shell.sound.recording(),
        [("cameras".to_owned(), RecordingDevice::Camera, true)],
        "shown using the camera from its start"
    );

    session.report(CaptureState::Interrupted(Some(
        CameraInterruption::Background,
    )));
    run.output.wait_for("state: interrupted (background)");
    session.report(CaptureState::Interrupted(Some(
        CameraInterruption::InUseByAnotherApp,
    )));
    run.output
        .wait_for("state: interrupted (in use by another app)");
    session.report(CaptureState::Interrupted(None));
    run.output.wait_for("state: interrupted");
    session.report(CaptureState::Running);
    run.output
        .wait_until("the program hears it runs again", |lines| {
            lines
                .iter()
                .filter(|line| *line == "state: running")
                .count()
                == 2
        });
    session.report(CaptureState::Ended(CameraError::Unplugged));
    run.output.wait_for("state: ended (unplugged)");
    let end = within(run.running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert!(session.is_dropped(), "let go as the program returns");
    assert_eq!(
        shell.sound.recording(),
        [
            ("cameras".to_owned(), RecordingDevice::Camera, true),
            ("cameras".to_owned(), RecordingDevice::Camera, false)
        ],
        "shown using the camera until it let the session go"
    );
}

#[test]
fn a_simulated_answer_opens_the_shells_own_camera_unasked_and_shows_it_simulated() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let run = Run::start(&engine, Run::answered(&shell, Answer::Simulated));
    run.output
        .wait_for("camera waos-test-pattern: WaOS test pattern (external)");
    run.output.wait_for("state: running");
    assert!(shell.prompts.questions().is_empty(), "nothing asked");
    assert!(
        shell.cameras.opened().is_empty(),
        "the system's camera untouched"
    );
    assert_eq!(
        shell.simulated_cameras.opened(),
        [("cameras".to_owned(), "waos-test-pattern".to_owned())]
    );
    assert_eq!(
        shell.sound.recording(),
        [("cameras".to_owned(), RecordingDevice::SimulatedCamera, true)],
        "shown with the camera of WaOS's own"
    );
    let session = shell.simulated_cameras.started().remove(0);
    session.report(CaptureState::Ended(CameraError::Unplugged));
    within(run.running.ended());
}

#[test]
fn a_simulated_answer_on_a_shell_without_cameras_of_its_own_lists_none() {
    let shell = TestShell::new();
    let mut parts = shell.shell();
    parts.simulated_cameras = None;
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let run = Run::start(&engine, Run::answered(&shell, Answer::Simulated));
    within(run.running.ended());
    assert!(
        !run.output
            .lines()
            .iter()
            .any(|line| line.starts_with("camera ")),
        "{:#?}",
        run.output.lines()
    );
    assert!(shell.cameras.opened().is_empty());
}

#[test]
fn the_program_hears_the_list_again_each_time_a_camera_comes_or_goes() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let run = Run::start(&engine, Run::allowed(&shell));
    run.output.wait_for("state: running");
    let cameras = listed();
    shell.cameras.set_listed(vec![cameras[1].clone()]);
    run.output.wait_for("cameras now: desk");
    shell.cameras.set_listed(cameras);
    run.output.wait_for("cameras now: front, desk");
    run.running.stop();
    within(run.running.ended());
}

#[test]
fn a_running_camera_keeps_its_program_from_freezing_until_it_lets_the_session_go() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let run = Run::start(&engine, Run::allowed(&shell));
    run.output.wait_for("state: running");
    within(engine.suspend());
    assert!(
        run.running.frozen().now_or_never().is_none(),
        "paused with its camera running, it is not frozen"
    );
    started(&shell).report(CaptureState::Ended(CameraError::Unplugged));
    run.output.wait_for("state: ended (unplugged)");
    let end = within(run.running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    engine.resume();
}

#[test]
fn a_refused_camera_opens_nothing_and_the_program_hears_why() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let run = Run::start(&engine, Run::asking(&shell));
    shell.changes.wait_until("the camera question is put", || {
        !shell.prompts.questions().is_empty()
    });
    shell.prompts.answer_questions(Some(Answer::Denied));
    run.output.wait_for("open front: not allowed");
    within(run.running.ended());
    assert!(shell.cameras.opened().is_empty());
    assert!(shell.sound.recording().is_empty());
}

#[test]
fn a_session_on_a_camera_running_for_someone_else_says_the_format_it_joined() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.cameras.run_elsewhere_in(formats()[0].clone());
    let run = Run::start(&engine, Run::allowed(&shell));
    run.output.wait_for("started in 1920x1080");
    let session = started(&shell);
    assert_eq!(sizes(&[session.picked.size]), ["640x480"], "the one picked");
    assert!(
        shell.prompts.questions().is_empty(),
        "answered before, so not asked"
    );
    run.running.stop();
    within(run.running.ended());
    assert!(session.is_dropped(), "let go as the program is stopped");
}

#[test]
fn a_format_the_camera_does_not_list_traps_the_program() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let run = Run::start_with(&engine, Run::allowed(&shell), &["unlisted"]);
    let end = within(run.running.ended());
    let ProgramEnd::Failed(trap) = end else {
        panic!("starting in an unlisted format traps: {end:?}");
    };
    assert!(
        format!("{trap:#}").contains("a format the camera does not list"),
        "{trap:#}"
    );
    assert!(
        shell.cameras.started().is_empty(),
        "nothing reached the shell"
    );
}

#[test]
fn a_shell_without_cameras_or_without_an_indicator_gives_the_program_none() {
    for adjust in [
        |parts: &mut waos::Shell| {
            parts.cameras = None;
            parts.simulated_cameras = None;
        },
        |parts: &mut waos::Shell| parts.recording_indicator = None,
    ] {
        let shell = TestShell::new();
        let mut parts = shell.shell();
        adjust(&mut parts);
        let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
        let run = Run::start(&engine, Run::allowed(&shell));
        assert!(matches!(
            within(run.running.ended()),
            ProgramEnd::Returned(Ok(()))
        ));
        assert_eq!(run.output.lines(), ["cameras: none"]);
    }
}

/// The session started first, once the shell has it.
fn started(shell: &TestShell) -> Started {
    shell.changes.wait_until("the shell starts a session", || {
        !shell.cameras.started().is_empty()
    });
    shell.cameras.started()[0].clone()
}

/// Each of `sizes` as the program writes one.
fn sizes(sizes: &[waos::bindings::wasdk::platform::windows::Size]) -> Vec<String> {
    sizes
        .iter()
        .map(|size| format!("{}x{}", size.width, size.height))
        .collect()
}

/// `examples/cameras` running with grants of the test's, in folders of its own.
struct Run {
    running: RunningProgram,
    output: Arc<RecordedOutput>,
    _folders: TempDir,
}

impl Run {
    fn start(engine: &WaOS, grants: Arc<dyn ProgramGrants>) -> Run {
        Run::start_with(engine, grants, &[])
    }

    /// [`start`](Self::start), with an empty file of each of `files` in its `/data` first.
    fn start_with(engine: &WaOS, grants: Arc<dyn ProgramGrants>, files: &[&str]) -> Run {
        let folders = TempDir::new().unwrap();
        let own = ProgramFolders {
            data: folders.path().join("data"),
            cache: folders.path().join("cache"),
        };
        fs::create_dir_all(&own.data).unwrap();
        fs::create_dir_all(&own.cache).unwrap();
        for file in files {
            fs::write(own.data.join(file), "").unwrap();
        }
        let output = Arc::new(RecordedOutput::default());
        let environment = ProgramEnvironment {
            grants: Some(grants),
            folders: Some(own),
            ..printing_to(&output)
        };
        let program = engine
            .load(
                Path::new(waos_test_programs::CAMERAS),
                &waos_test_programs::code_of(waos_test_programs::CAMERAS),
            )
            .unwrap();
        let running = engine.run(&program, environment).unwrap();
        Run {
            running,
            output,
            _folders: folders,
        }
    }

    /// Grants whose user, `shell`'s, has not answered the camera question yet.
    fn asking(shell: &TestShell) -> Arc<dyn ProgramGrants> {
        shell.permissions().of(&app())
    }

    /// Grants whose user allowed the camera before.
    fn allowed(shell: &TestShell) -> Arc<dyn ProgramGrants> {
        Run::answered(shell, Answer::Granted)
    }

    /// Grants whose user answered the camera question `answer` before.
    fn answered(shell: &TestShell, answer: Answer) -> Arc<dyn ProgramGrants> {
        let permissions = permissions_with(shell, |own| {
            own.answers.set(Question::Camera, Setting::Own(answer));
        });
        permissions.of(&app())
    }
}
