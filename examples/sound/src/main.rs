//! A program that makes sound, and what a reader learns the `audio` interface from: it makes a
//! context and, through its base, a graph of every kind of node, connected from an oscillator to
//! the destination, and plays the oscillator for a moment; decodes a sound file it makes itself,
//! and is refused one that would decode to more than a buffer holds; renders a graph on an offline
//! context into a buffer; writes samples ahead of its graph through a stream input and hears them
//! come back through an analyser; asks the user to pick an output and moves onto it; opens the
//! microphone, which asks the user, and hears it; and closes the context.
//!
//! It prints each step as `what: result`, a refusal by the error's case.
//!
//! ```text
//! cargo build -p sound --target wasm32-wasip2
//! ```

use wasdk::audio::{
    self, AnalyserNode, Audio, AudioContext, AudioContextOptions, AudioError, BaseAudioContext,
    BiquadFilterType, OfflineAudioContext, OscillatorType,
};
use wasdk::monotonic_clock;

/// The value the streamed samples hold, which the analyser listens for.
const STREAMED: f32 = 0.5;

/// How many times a step listens before it gives up.
const TRIES: u32 = 400;

/// The frames of a file of six hours at one frame a second: 45 kB, which decodes to 4 GiB of
/// samples at 48 kHz.
const HOURS: usize = 22_400;

/// How long the microphone is listened to between two looks: 10 ms, in nanoseconds.
const LOOK_EVERY: u64 = 10_000_000;

#[wasdk::main]
async fn main() {
    let Some(audio) = audio::get_audio() else {
        println!("no sound");
        return;
    };
    let context = match audio.new_context(AudioContextOptions::new()) {
        Ok(context) => context,
        Err(error) => return println!("context refused: {}", said(&error)),
    };
    let base = context.base();
    println!("sample rate: {}", base.sample_rate());
    play_every_node(&base).await;
    decode(&base).await;
    render_offline().await;
    stream(&context).await;
    pick_output(&audio, &context).await;
    listen_to_the_microphone(&context).await;
    context.close().await;
    println!("closed: {:?}", base.state());
}

/// Connects a node of every kind, from an oscillator to the destination, plays the oscillator
/// for a moment, and prints what the nodes say of themselves.
async fn play_every_node(context: &BaseAudioContext) {
    let oscillator = context.create_oscillator();
    oscillator.set_type(OscillatorType::Triangle);
    let wave = context.create_periodic_wave(&[0.0, 1.0], &[0.0, 0.0], false);
    oscillator.set_periodic_wave(&wave);
    let constant = context.create_constant_source();
    constant.offset().set_value(3.0);
    constant.node().connect_param(&oscillator.detune());

    let filter = context.create_biquad_filter();
    filter.set_type(BiquadFilterType::Highpass);
    filter.frequency().linear_ramp_to_value_at_time(880.0, 0.5);
    let iir = context.create_iir_filter(&[0.5, 0.5], &[1.0]);
    let compressor = context.create_dynamics_compressor();
    let convolver = context.create_convolver();
    let impulse = context.create_buffer(1, 4, context.sample_rate());
    impulse.copy_to_channel(&[1.0, 0.0, 0.0, 0.0], 0, 0);
    convolver.set_buffer(&impulse);
    let shaper = context.create_wave_shaper();
    shaper.set_curve(&[-1.0, 0.0, 1.0]);
    let stereo = context.create_stereo_panner();
    stereo.pan().set_value(-0.5);
    let panner = context.create_panner();
    panner.position_x().set_value(1.0);
    context.listener().position_z().set_value(1.0);
    let delay = context.create_delay(1.0);
    delay.delay_time().set_value_at_time(0.25, 0.0);
    let splitter = context.create_channel_splitter(2);
    let merger = context.create_channel_merger(2);
    let gain = context.create_gain();
    gain.gain().set_target_at_time(0.5, 0.0, 0.1);
    let analyser = context.create_analyser();
    let buffer_source = context.create_buffer_source();
    buffer_source.set_buffer(&impulse);
    buffer_source.set_loop(true);
    let destination = context.destination();

    let chain = [
        oscillator.node(),
        filter.node(),
        iir.node(),
        compressor.node(),
        convolver.node(),
        shaper.node(),
        stereo.node(),
        panner.node(),
        delay.node(),
        splitter.node(),
    ];
    for pair in chain.windows(2) {
        pair[0].connect(&pair[1]);
    }
    splitter.node().connect_output_input(&merger.node(), 1, 0);
    merger.node().connect(&gain.node());
    buffer_source.node().connect(&gain.node());
    gain.node().connect(&analyser.node());
    analyser.node().connect(&destination.node());

    println!(
        "graph: gain {}, filter {:?}, {} bins, {} channels at most, merger has {} inputs",
        gain.gain().default_value(),
        filter.type_(),
        analyser.frequency_bin_count(),
        destination.max_channel_count(),
        merger.node().number_of_inputs(),
    );

    oscillator.start(0.0);
    constant.start(0.0);
    buffer_source.start(0.0, 0.0, None);
    oscillator.stop(context.current_time() + 0.05);
    oscillator.ended().await;
    constant.stop(0.0);
    buffer_source.stop(0.0);
    constant.ended().await;
    buffer_source.ended().await;
    println!("played: every source ended");
}

/// Decodes a sound file of 64 frames of 16-bit samples, made here, at the context's rate; noise;
/// and a small file of six hours at one frame a second, more than a buffer holds at 48 kHz.
async fn decode(context: &BaseAudioContext) {
    let rate = context.sample_rate() as u32;
    match context.decode_audio_data(wav(rate, &[8192; 64])).await {
        Ok(buffer) => println!(
            "decoded: {} channel, {} frames, first {}",
            buffer.number_of_channels(),
            buffer.length(),
            buffer.get_channel_data(0)[0]
        ),
        Err(error) => println!("decode refused: {}", said(&error)),
    }
    match context.decode_audio_data(b"no sound here".to_vec()).await {
        Ok(_) => println!("noise decoded"),
        Err(error) => println!("noise refused: {}", said(&error)),
    }
    match context.decode_audio_data(wav(1, &[0; HOURS])).await {
        Ok(_) => println!("hours decoded"),
        Err(error) => println!("hours refused: {}", said(&error)),
    }
}

/// Renders a constant source of 0.5 through a gain of 0.5 on an offline context, a block's
/// length.
async fn render_offline() {
    let offline = OfflineAudioContext::new(1, 128, 48_000.0);
    let base = offline.base();
    let source = base.create_constant_source();
    source.offset().set_value(0.5);
    let gain = base.create_gain();
    gain.gain().set_value(0.5);
    source.node().connect(&gain.node());
    gain.node().connect(&base.destination().node());
    source.start(0.0);
    let rendered = offline.start_rendering().await;
    println!(
        "rendered: {} channel, {} frames, first {}, {:?}",
        rendered.number_of_channels(),
        rendered.length(),
        rendered.get_channel_data(0)[0],
        base.state()
    );
}

/// Writes samples of [`STREAMED`] ahead of the graph until the analyser hears them.
async fn stream(context: &AudioContext) {
    let input = context.create_stream_input(2);
    let base = context.base();
    let analyser = base.create_analyser();
    input.node().connect(&analyser.node());
    analyser.node().connect(&base.destination().node());
    let block = vec![STREAMED; 2 * 512];
    for _ in 0..TRIES {
        if let Err(error) = input.write(block.clone()).await {
            return println!("stream refused: {}", said(&error));
        }
        if hears(&analyser, STREAMED) {
            return println!("stream: heard {STREAMED} at {} channels", input.channels());
        }
    }
    println!("stream: not heard");
}

/// Asks the user to pick an output, moves onto it, and tries one the user did not pick.
async fn pick_output(audio: &Audio, context: &AudioContext) {
    let picked = match audio.select_audio_output().await {
        Ok(picked) => picked,
        Err(error) => return println!("pick refused: {}", said(&error)),
    };
    println!("picked: {}", picked.label);
    match context.set_sink_id(picked.device_id).await {
        Ok(()) => println!("moved: {}", context.sink_id()),
        Err(error) => println!("move refused: {}", said(&error)),
    }
    if let Err(error) = context
        .set_sink_id("an output nobody picked".to_owned())
        .await
    {
        println!("other output refused: {}", said(&error));
    }
}

/// Opens the microphone, hears it, and lets it go.
async fn listen_to_the_microphone(context: &AudioContext) {
    let microphone = match context.open_microphone().await {
        Ok(microphone) => microphone,
        Err(error) => return println!("microphone refused: {}", said(&error)),
    };
    let analyser = context.base().create_analyser();
    microphone.node().connect(&analyser.node());
    for _ in 0..TRIES {
        if hears(&analyser, 0.0) {
            println!("microphone: heard");
            break;
        }
        monotonic_clock::wait_for(LOOK_EVERY).await;
    }
    drop(microphone);
    println!("microphone: let go");
}

/// Whether `analyser` holds a sample other than silence, near `value` unless that is silence.
fn hears(analyser: &AnalyserNode, value: f32) -> bool {
    analyser
        .get_float_time_domain_data()
        .iter()
        .any(|sample| *sample != 0.0 && (value == 0.0 || (sample - value).abs() < 1e-3))
}

/// A WAV file of `samples`, one channel at `rate`.
fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_size = (samples.len() * 2) as u32;
    let mut file = Vec::with_capacity(44 + samples.len() * 2);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data_size).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&rate.to_le_bytes());
    file.extend_from_slice(&(rate * 2).to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        file.extend_from_slice(&sample.to_le_bytes());
    }
    file
}

/// An error, by its case.
fn said(error: &AudioError) -> &'static str {
    match error {
        AudioError::NotAllowed => "not-allowed",
        AudioError::NotFound => "not-found",
        AudioError::NotSupported(_) => "not-supported",
        AudioError::Encoding(_) => "encoding",
        AudioError::InvalidState(_) => "invalid-state",
    }
}
