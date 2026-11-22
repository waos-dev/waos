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

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use waos::{
    Answer, AudioDevices, EngineTime, FreezePolicy, ManualTime, Presence, ProgramEnd,
    ProgramEnvironment, ProgramGrants, Question, RecordingDevice, RunningProgram, WaOS, WaOSConfig,
};

use crate::asking::QuestionsAsked;
use crate::test_shell::sound::{HEADPHONES, TestDevices};
use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, wait_until, within};

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
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let (output, devices, running) = run_sound(&engine);
    for line in BEFORE_THE_MICROPHONE {
        output.wait_for(line);
    }
    assert_eq!(shell.sound.shown(), [["speakers", HEADPHONES]]);
    assert_eq!(devices.moves(), [HEADPHONES]);

    wait_until("the microphone question is put", || {
        !shell.prompts.questions().is_empty()
    });
    assert_eq!(
        shell.prompts.questions(),
        [("sound".to_owned(), Question::Microphone)]
    );
    shell.prompts.answer_questions(Some(Answer::Granted));
    output.wait_for("microphone: heard");
    output.wait_for("microphone: let go");
    output.wait_for("closed: AudioContextState::Closed");
    let end = within(running.ended());
    assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
    assert_eq!(
        shell.sound.recording(),
        [
            ("sound".to_owned(), RecordingDevice::Microphone, true),
            ("sound".to_owned(), RecordingDevice::Microphone, false)
        ],
        "shown recording from its open to its let go"
    );
}

#[test]
fn a_dismissed_picker_and_a_refused_microphone_say_why_and_nothing_records() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.sound.set_pick(None);
    let (output, devices, running) = run_sound(&engine);
    output.wait_for("pick refused: not-allowed");
    wait_until("the microphone question is put", || {
        !shell.prompts.questions().is_empty()
    });
    shell.prompts.answer_questions(Some(Answer::Denied));
    output.wait_for("microphone refused: not-allowed");
    within(running.ended());
    assert!(devices.moves().is_empty());
    assert!(shell.sound.recording().is_empty());
}

#[test]
fn an_output_set_as_the_pickers_answer_is_given_unasked_and_one_not_connected_is_not_found() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let grants = QuestionsAsked::answering_the_output_picker_with(HEADPHONES);
    let (output, devices, running) = run_sound_with(&engine, TestDevices::default(), grants);
    output.wait_for("picked: Headphones");
    output.wait_for("moved: headphones");
    running.stop();
    within(running.ended());
    assert!(shell.sound.shown().is_empty(), "no picker shown");
    assert_eq!(devices.moves(), [HEADPHONES]);

    let grants = QuestionsAsked::answering_the_output_picker_with("unplugged");
    let (output, devices, running) = run_sound_with(&engine, TestDevices::default(), grants);
    output.wait_for("pick refused: not-found");
    running.stop();
    within(running.ended());
    assert!(
        shell.sound.shown().is_empty(),
        "no picker shown in its place"
    );
    assert!(devices.moves().is_empty());
}

#[test]
fn a_waiting_output_picker_is_answered_by_setting_its_row_and_drops_its_prompt() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    shell.sound.hold_picks();
    let grants = Arc::new(QuestionsAsked::default());
    let program = engine.load(Path::new(waos_test_programs::SOUND)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let devices = Arc::new(TestDevices::default());
    let environment = ProgramEnvironment {
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        audio: Some(Arc::clone(&devices) as Arc<dyn AudioDevices>),
        ..printing_to(&output)
    };
    let running = engine.run(&program, environment).unwrap();

    wait_until("output picker is waiting", || {
        !shell.sound.shown().is_empty()
    });
    assert!(!shell.sound.held_cancelled(0));

    grants.set_audio_output_answer(HEADPHONES);
    wait_until("output picker future is dropped", || {
        shell.sound.held_cancelled(0)
    });
    output.wait_for("picked: Headphones");
    output.wait_for("moved: headphones");
    running.stop();
    within(running.ended());
}

#[test]
fn a_simulated_microphone_records_waoss_tone_unasked_on_devices_without_one_and_shows_simulated() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let grants = QuestionsAsked::default();
    grants.set_answer(Question::Microphone, Answer::Simulated);
    let (output, _devices, running) =
        run_sound_with(&engine, TestDevices::without_microphone(), grants);
    output.wait_for("microphone: heard");
    output.wait_for("microphone: let go");
    within(running.ended());
    assert!(shell.prompts.questions().is_empty(), "nothing asked");
    assert_eq!(
        shell.sound.recording(),
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
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let grants = QuestionsAsked::silent_simulated_microphone();
    let (output, _devices, running) = run_sound_with(&engine, TestDevices::default(), grants);
    output.wait_for("microphone: let go");
    within(running.ended());
    assert!(!output.has("microphone: heard"), "{:#?}", output.lines());
}

#[test]
fn a_shell_with_no_picker_and_no_indicator_offers_neither_and_asks_nothing() {
    let shell = TestShell::new();
    let mut parts = shell.shell();
    parts.audio_output_picker = None;
    parts.recording_indicator = None;
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let (output, _devices, running) = run_sound(&engine);
    output.wait_for("pick refused: not-supported");
    output.wait_for("microphone refused: not-supported");
    within(running.ended());
    assert!(
        shell.prompts.questions().is_empty(),
        "nothing asked: nobody could see it record"
    );
}

#[test]
fn devices_without_a_microphone_refuse_it_and_ask_nothing() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let (output, _devices, running) = run_sound_on(&engine, TestDevices::without_microphone());
    output.wait_for("microphone refused: not-supported");
    within(running.ended());
    assert!(
        shell.prompts.questions().is_empty(),
        "nothing asked: there is no microphone to give"
    );
    assert!(shell.sound.recording().is_empty());
}

#[test]
fn a_program_given_no_sound_devices_has_no_sound() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine.load(Path::new(waos_test_programs::SOUND)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let environment = printing_to(&output);
    let running = engine.run(&program, environment).unwrap();
    output.wait_for("no sound");
    within(running.ended());
}

/// A view taken off screen, as the user minimizing its window does.
const HIDDEN: Presence = Presence {
    on_screen: false,
    occluded: false,
    focused: false,
};

/// A view on screen again.
const SHOWN: Presence = Presence {
    on_screen: true,
    occluded: false,
    focused: false,
};

/// How long `examples/tone` stays hidden before the engine freezes it: three ticks.
const AFTER_HIDDEN: Duration = Duration::from_millis(300);

#[test]
fn a_program_is_thawed_while_it_plays_frozen_while_its_tone_waits_and_trapped_starting_twice() {
    let time = ManualTime::new();
    let policy = FreezePolicy {
        after_hidden: Some(AFTER_HIDDEN),
    };
    let shell = TestShell::new();
    let config = WaOSConfig {
        time: EngineTime::Manual(time.clone()),
        freeze: policy,
        ..config()
    };
    let engine = WaOS::new(config, shell.shell(), shell.platform()).unwrap();
    let program = engine.load(Path::new(waos_test_programs::TONE)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let environment = ProgramEnvironment {
        audio: Some(Arc::new(TestDevices::default()) as Arc<dyn AudioDevices>),
        ..printing_to(&output)
    };
    let running = engine.run(&program, environment).unwrap();
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("tone asks for a view");
    output.wait_for("playing");

    view.presence_changed(HIDDEN);
    output.wait_for("hidden, playing");
    time.elapse(AFTER_HIDDEN * 3);
    assert!(
        running.frozen().now_or_never().is_none(),
        "audible, it is not frozen"
    );

    view.presence_changed(SHOWN);
    output.wait_for("shown, ended, another an hour ahead");
    view.presence_changed(HIDDEN);
    output.wait_for("hidden, silent until then");
    time.elapse(AFTER_HIDDEN);
    within(running.frozen());

    view.presence_changed(SHOWN);
    let end = within(running.ended());
    let ProgramEnd::Failed(trap) = end else {
        panic!("starting its tone twice traps it: {end:?}");
    };
    assert!(
        format!("{trap:#}").contains("InvalidStateError - Cannot call `start` twice"),
        "{trap:#}"
    );
    assert!(!output.has("started twice"));
}

/// Runs `examples/sound` on `engine` with devices of its own and grants that ask about the
/// microphone, and answers what it prints, the devices and the program.
fn run_sound(engine: &WaOS) -> (Arc<RecordedOutput>, Arc<TestDevices>, RunningProgram) {
    run_sound_on(engine, TestDevices::default())
}

/// [`run_sound`], on `devices`.
fn run_sound_on(
    engine: &WaOS,
    devices: TestDevices,
) -> (Arc<RecordedOutput>, Arc<TestDevices>, RunningProgram) {
    run_sound_with(engine, devices, QuestionsAsked::default())
}

/// [`run_sound`], on `devices`, with `grants`.
fn run_sound_with(
    engine: &WaOS,
    devices: TestDevices,
    grants: QuestionsAsked,
) -> (Arc<RecordedOutput>, Arc<TestDevices>, RunningProgram) {
    let program = engine.load(Path::new(waos_test_programs::SOUND)).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let devices = Arc::new(devices);
    let environment = ProgramEnvironment {
        grants: Some(Arc::new(grants) as Arc<dyn ProgramGrants>),
        audio: Some(Arc::clone(&devices) as Arc<dyn AudioDevices>),
        ..printing_to(&output)
    };
    let running = engine.run(&program, environment).unwrap();
    (output, devices, running)
}
