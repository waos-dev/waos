//! `examples/video-decoders` under the test shell: a program hands the system's decoder chunks
//! and reads its frames. The engine keeps the rules every system shares, so the shell's decoder
//! never sees them broken: a chunk that is no key frame reaches it only once a key frame has,
//! since it was opened, drained, reset or told of damage; and a program holding as many frames as
//! it said it would gets no more, and the shell is asked for none, until it drops one. A frame
//! says what its decoder said of it and is the decoder's own picture, two planes the program
//! reads each through a texture and copies into its memory, and a decoder the system takes away says so beside its frames.

use std::path::Path;
use std::sync::Arc;

use waos::bindings::wasdk::media::video_streams::{NalFormat, VideoStreamSetup};
use waos::{ProgramEnd, WaOS};

use crate::test_shell::video_decoders::DecoderCall;
use crate::test_shell::{RecordedOutput, TestShell, config, printing_to, wait_until, within};

/// What the engine answers a chunk that is no key frame while the decoder waits for one.
const KEY_FRAME_DUE: &str = r#"Err(DecoderError::BadData("the decoder takes a key frame next"))"#;

#[test]
fn a_decoder_takes_key_frames_first_lends_no_more_frames_than_asked_and_says_why_it_ended() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::VIDEO_DECODERS))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&program, printing_to(&output)).unwrap();

    output.wait_for("waiting for the decoder to end");
    assert_eq!(
        shell.gpus.asked().len(),
        1,
        "the program asked once for the decoders of its device's GPU"
    );
    let decoder = Arc::clone(&shell.gpus.video_decoders.opened()[0]);
    assert!(matches!(
        decoder.setup,
        VideoStreamSetup::H264(NalFormat::StartCodes(None))
    ));
    assert_eq!(decoder.options.frame_limit, 2);
    // The program itself tells that no third frame came while it held two (its lines, below).
    // Here: the shell was asked for the six frames the program read, and none ahead of it.
    assert_eq!(decoder.frames_taken(), 6);
    assert_eq!(
        decoder.calls(),
        [
            DecoderCall::Decode(0),
            DecoderCall::Decode(1),
            DecoderCall::Decode(2),
            DecoderCall::Decode(3),
            DecoderCall::Drain,
            DecoderCall::Decode(5),
            DecoderCall::Decode(6),
            DecoderCall::Reset,
            DecoderCall::Decode(9),
        ],
        "no chunk the engine refused reached the decoder"
    );
    decoder.lose();

    let end = within(running.ended());
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "{end:?}: {:?}",
        output.lines()
    );
    assert_eq!(
        output.lines(),
        [
            "support: hardware DecodeSupport::Unknown, software DecodeSupport::Yes".to_owned(),
            "opened a CodecKind::Software decoder".to_owned(),
            format!("a chunk before any key frame: {KEY_FRAME_DUE}"),
            "frames Some(0) and Some(1) held: no third while they are".to_owned(),
            "after one is dropped: frame Some(2)".to_owned(),
            "frame Some(3) for Some(40): 60x40 of a 64x48 buffer from 2,4, pixels 4 to 3, \
             primaries Some(1)"
                .to_owned(),
            "plane 0: PlaneContent::Luma as PlaneFormat::R8, 64x48 of 8 bits, first sample [63]"
                .to_owned(),
            "plane 1: PlaneContent::Chroma as PlaneFormat::Rg8, 32x24 of 8 bits, first sample \
             [102, 240]"
                .to_owned(),
            "plane 0 in memory: 48 rows, every sample [63]".to_owned(),
            "plane 1 in memory: 24 rows, every sample [102, 240]".to_owned(),
            format!("a chunk that is no key frame after a drain: {KEY_FRAME_DUE}"),
            r#"a damaged chunk: Err(DecoderError::BadData("the test's damaged chunk"))"#.to_owned(),
            format!("the chunk after it: {KEY_FRAME_DUE}"),
            format!("a chunk that is no key frame after a reset: {KEY_FRAME_DUE}"),
            "a key frame goes on: frame Some(9)".to_owned(),
            "waiting for the decoder to end".to_owned(),
            "the decoder ended: Err(DecoderError::Lost)".to_owned(),
        ]
    );
    wait_until(
        "the engine lets go of the decoder as the program ends",
        || Arc::strong_count(&decoder) == 2,
    );
}

#[test]
fn a_shell_without_video_decoders_gives_the_program_none() {
    let shell = TestShell::new();
    let mut parts = shell.shell();
    parts.gpus = None;
    let engine = WaOS::new(config(), parts, shell.platform()).unwrap();
    let program = engine
        .load(Path::new(waos_test_programs::VIDEO_DECODERS))
        .unwrap();
    let output = Arc::new(RecordedOutput::default());
    let running = engine.run(&program, printing_to(&output)).unwrap();
    assert!(matches!(
        within(running.ended()),
        ProgramEnd::Returned(Ok(()))
    ));
    assert_eq!(output.lines(), ["video decoders: none"]);
}
