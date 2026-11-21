//! Sound under the test shell, on sound devices of the test's own whose contexts play on no output,
//! so no test opens the machine's: `examples/sound` builds a graph of every kind of node and plays
//! it, decodes a file and is refused one too long, renders a graph offline, hears what it streams,
//! picks an output and moves onto it, and hears the microphone once its user allows it, which the
//! shell shows; a program refused the picker or the microphone hears why, and is asked nothing for
//! devices with no microphone, unless its microphone is simulated, which records WaOS's quiet tone,
//! or silence as chosen, shown apart; a program whose user set the picker's answer is given that output
//! unasked, and one set that is not connected is not found; a program given no
//! devices has no sound; and `examples/tone`, on time the test moves, is not frozen while it plays
//! out of sight, is frozen once silent, a tone scheduled an hour ahead keeping nothing thawed, and
//! traps when it starts its tone twice.
//!
//! The graph renders on web-audio-api's thread in time, as on a device, so a step that listens
//! to it waits for what it hears; the tests wait for what the programs print.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::executor::block_on;
use waos::{
    Answer, AudioDevices, FreezePolicy, ManualTime, ProgramEnd, ProgramEnvironment, ProgramGrants,
    ProgramOutput, Question, RecordingDevice,
};

use crate::asking::QuestionsAsked;
use crate::shell::sound::{HEADPHONES, TestDevices};
use crate::shell::{self, Lines, WITHIN, hide, show, wait_until_frozen};

/// What `examples/sound` prints before it opens the microphone, in order.
const BEFORE_THE_MICROPHONE: [&str; 11] = [
    "sample rate: 48000",
    "graph: gain 1, filter BiquadFilterType::Highpass, 1024 bins, 32 channels at most, merger has 2 inputs",
    "played: every source ended",
    "decoded: 1 channel, 64 frames, first 0.25",
    "noise refused: encoding",
    "hours refused: not-supported",
    "rendered: 1 channel, 128 frames, first 0.25, AudioContextState::Closed",
    "stream: heard 0.5 at 2 channels",
    "picked: Headphones",
    "moved: headphones",
    "other output refused: not-allowed",
];

#[test]
fn a_program_plays_every_node_streams_picks_an_output_and_records_once_allowed() {
    let (engine, shell, _arrivals) = shell::engine();
    let (lines, devices, running) = run_sound(&engine);
    for line in BEFORE_THE_MICROPHONE {
        lines.wait_for(line);
    }
    assert_eq!(shell.sound().shown(), [["speakers", HEADPHONES]]);
    assert_eq!(devices.moves(), [HEADPHONES]);

    wait_until("the microphone question is put", || {
        !shell.questions().is_empty()
    });
    assert_eq!(
        shell.questions(),
        [("sound".to_owned(), Question::Microphone)]
    );
    shell.answer_questions(Some(Answer::Granted));
    lines.wait_for("microphone: heard");
    lines.wait_for("microphone: let go");
    lines.wait_for("closed: AudioContextState::Closed");
    let end = block_on(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert_eq!(
        shell.sound().recording(),
        [
            ("sound".to_owned(), RecordingDevice::Microphone, true),
            ("sound".to_owned(), RecordingDevice::Microphone, false)
        ],
        "shown recording from its open to its let go"
    );
}

#[test]
fn a_dismissed_picker_and_a_refused_microphone_say_why_and_nothing_records() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.sound().set_pick(None);
    let (lines, devices, running) = run_sound(&engine);
    lines.wait_for("pick refused: not-allowed");
    wait_until("the microphone question is put", || {
        !shell.questions().is_empty()
    });
    shell.answer_questions(Some(Answer::Denied));
    lines.wait_for("microphone refused: not-allowed");
    block_on(running.ended());
    assert!(devices.moves().is_empty());
    assert!(shell.sound().recording().is_empty());
}

#[test]
fn an_output_set_as_the_pickers_answer_is_given_unasked_and_one_not_connected_is_not_found() {
    let (engine, shell, _arrivals) = shell::engine();
    let grants = QuestionsAsked::answering_the_output_picker_with(HEADPHONES);
    let (lines, devices, running) = run_sound_with(&engine, TestDevices::default(), grants);
    lines.wait_for("picked: Headphones");
    lines.wait_for("moved: headphones");
    running.stop();
    block_on(running.ended());
    assert!(shell.sound().shown().is_empty(), "no picker shown");
    assert_eq!(devices.moves(), [HEADPHONES]);

    let grants = QuestionsAsked::answering_the_output_picker_with("unplugged");
    let (lines, devices, running) = run_sound_with(&engine, TestDevices::default(), grants);
    lines.wait_for("pick refused: not-found");
    running.stop();
    block_on(running.ended());
    assert!(
        shell.sound().shown().is_empty(),
        "no picker shown in its place"
    );
    assert!(devices.moves().is_empty());
}

#[test]
fn a_waiting_output_picker_is_answered_by_setting_its_row_and_drops_its_prompt() {
    let (engine, shell, _arrivals) = shell::engine();
    shell.sound().hold_picks();
    let grants = Arc::new(QuestionsAsked::default());
    let program = shell::load(&engine, waos_test_programs::SOUND);
    let lines = Arc::new(Lines::default());
    let devices = Arc::new(TestDevices::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        audio: Some(Arc::clone(&devices) as Arc<dyn AudioDevices>),
        ..ProgramEnvironment::default()
    };
    let running = engine.run(&program, environment).unwrap();

    wait_until("output picker is waiting", || {
        !shell.sound().shown().is_empty()
    });
    assert!(!shell.sound().held_cancelled(0));

    grants.set_audio_output_answer(HEADPHONES);
    wait_until("output picker future is dropped", || {
        shell.sound().held_cancelled(0)
    });
    lines.wait_for("picked: Headphones");
    lines.wait_for("moved: headphones");
    running.stop();
    block_on(running.ended());
}

#[test]
fn a_simulated_microphone_records_waoss_tone_unasked_on_devices_without_one_and_shows_simulated() {
    let (engine, shell, _arrivals) = shell::engine();
    let grants = QuestionsAsked::default();
    grants.set_answer(Question::Microphone, Answer::Simulated);
    let (lines, _devices, running) =
        run_sound_with(&engine, TestDevices::without_microphone(), grants);
    lines.wait_for("microphone: heard");
    lines.wait_for("microphone: let go");
    block_on(running.ended());
    assert!(shell.questions().is_empty(), "nothing asked");
    assert_eq!(
        shell.sound().recording(),
        [
            (
                "sound".to_owned(),
                RecordingDevice::SimulatedMicrophone,
                true
            ),
            (
                "sound".to_owned(),
                RecordingDevice::SimulatedMicrophone,
                false
            )
        ],
        "shown recording with the microphone of WaOS's own"
    );
}

#[test]
fn a_simulated_microphone_chosen_silent_is_heard_as_nothing() {
    let (engine, _shell, _arrivals) = shell::engine();
    let grants = QuestionsAsked::silent_simulated_microphone();
    let (lines, _devices, running) = run_sound_with(&engine, TestDevices::default(), grants);
    lines.wait_for("microphone: let go");
    block_on(running.ended());
    assert!(!lines.has("microphone: heard"), "{:#?}", lines.all());
}

#[test]
fn a_shell_with_no_picker_and_no_indicator_offers_neither_and_asks_nothing() {
    let (engine, shell, _arrivals) = shell::engine_of(shell::config(), |parts| {
        parts.audio_output_picker = None;
        parts.recording_indicator = None;
    });
    let (lines, _devices, running) = run_sound(&engine);
    lines.wait_for("pick refused: not-supported");
    lines.wait_for("microphone refused: not-supported");
    block_on(running.ended());
    assert!(
        shell.questions().is_empty(),
        "nothing asked: nobody could see it record"
    );
}

#[test]
fn devices_without_a_microphone_refuse_it_and_ask_nothing() {
    let (engine, shell, _arrivals) = shell::engine();
    let (lines, _devices, running) = run_sound_on(&engine, TestDevices::without_microphone());
    lines.wait_for("microphone refused: not-supported");
    block_on(running.ended());
    assert!(
        shell.questions().is_empty(),
        "nothing asked: there is no microphone to give"
    );
    assert!(shell.sound().recording().is_empty());
}

#[test]
fn a_program_given_no_sound_devices_has_no_sound() {
    let (engine, _shell, _arrivals) = shell::engine();
    let program = shell::load(&engine, waos_test_programs::SOUND);
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        ..ProgramEnvironment::default()
    };
    let running = engine.run(&program, environment).unwrap();
    lines.wait_for("no sound");
    block_on(running.ended());
}

/// How long `examples/tone` stays hidden before the engine freezes it: three ticks.
const AFTER_HIDDEN: Duration = Duration::from_millis(300);

#[test]
fn a_program_is_thawed_while_it_plays_frozen_while_its_tone_waits_and_trapped_starting_twice() {
    let time = ManualTime::new();
    let policy = FreezePolicy {
        after_hidden: Some(AFTER_HIDDEN),
    };
    let (engine, shell, _arrivals) = shell::engine_on(&time, policy);
    let program = shell::load(&engine, waos_test_programs::TONE);
    let lines = Arc::new(Lines::default());
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        audio: Some(Arc::new(TestDevices::default()) as Arc<dyn AudioDevices>),
        ..ProgramEnvironment::default()
    };
    let (running, view) = shell::run_on_reserved(&engine, &shell, &program, environment);
    let view = &view;
    lines.wait_for("playing");

    hide(view);
    lines.wait_for("hidden, playing");
    time.elapse(AFTER_HIDDEN * 3);
    assert!(
        running.frozen().now_or_never().is_none(),
        "audible, it is not frozen"
    );

    show(view);
    lines.wait_for("shown, ended, another an hour ahead");
    hide(view);
    lines.wait_for("hidden, silent until then");
    time.elapse(AFTER_HIDDEN);
    wait_until_frozen(&running);

    show(view);
    let end = block_on(running.ended());
    let ProgramEnd::Failed(trap) = end else {
        panic!("starting its tone twice traps it: {end:?}");
    };
    assert!(
        format!("{trap:#}").contains("InvalidStateError - Cannot call `start` twice"),
        "{trap:#}"
    );
    assert!(!lines.has("started twice"));
}

/// Runs `examples/sound` on `engine` with devices of its own and grants that ask about the
/// microphone, and answers what it prints, the devices and the program.
fn run_sound(engine: &waos::WaOS) -> (Arc<Lines>, Arc<TestDevices>, waos::RunningProgram) {
    run_sound_on(engine, TestDevices::default())
}

/// [`run_sound`], on `devices`.
fn run_sound_on(
    engine: &waos::WaOS,
    devices: TestDevices,
) -> (Arc<Lines>, Arc<TestDevices>, waos::RunningProgram) {
    run_sound_with(engine, devices, QuestionsAsked::default())
}

/// [`run_sound`], on `devices`, with `grants`.
fn run_sound_with(
    engine: &waos::WaOS,
    devices: TestDevices,
    grants: QuestionsAsked,
) -> (Arc<Lines>, Arc<TestDevices>, waos::RunningProgram) {
    let program = shell::load(engine, waos_test_programs::SOUND);
    let lines = Arc::new(Lines::default());
    let devices = Arc::new(devices);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        grants: Some(Arc::new(grants) as Arc<dyn ProgramGrants>),
        audio: Some(Arc::clone(&devices) as Arc<dyn AudioDevices>),
        ..ProgramEnvironment::default()
    };
    let running = engine.run(&program, environment).unwrap();
    (lines, devices, running)
}

/// Waits until `done`, failing after [`WITHIN`].
fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WITHIN;
    while !done() {
        assert!(Instant::now() < deadline, "{what} never happened");
        thread::sleep(Duration::from_millis(5));
    }
}
