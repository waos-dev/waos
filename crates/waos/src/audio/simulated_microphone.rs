//! The microphone of WaOS's own that a program whose microphone answer is simulated records from:
//! a stream WaOS makes, touching no device, of what the user chose (`SimulatedSound`).
//!
//! The tone follows Chrome's fake audio input (`media/audio/simple_sources.cc`'s `BeepingSource`,
//! which `--use-fake-device-for-media-stream` gives a page): a 400 Hz beep 20 ms long every
//! 500 ms. Chrome's is a full-scale square wave; this one is a sine at a twentieth of full scale
//! (-26 dBFS), its 2 ms edges shaped, so a person can bear it for hours and a test can still find
//! each beep and its pitch. Silence is the same stream at zero.
//!
//! The invariant: the stream is made of render quanta asked for one at a time, as a device's is,
//! so it plays in time with the context that reads it, however fast it could be made.

use std::f64::consts::TAU;

use waos_capabilities::SimulatedSound;
use waos_capabilities::web_audio_api::AudioBuffer;
use waos_capabilities::web_audio_api::media_streams::{MediaStream, MediaStreamTrack};

/// The rate the stream is made at; the context reading it resamples to its own.
pub(crate) const SAMPLE_RATE: f32 = 48_000.0;
/// The beep's pitch, Chrome's.
pub(crate) const TONE_HZ: f64 = 400.0;
/// How long a beep lasts, and how often one starts, in samples: Chrome's 20 ms every 500 ms.
pub(crate) const BEEP_SAMPLES: u64 = 960;
pub(crate) const PERIOD_SAMPLES: u64 = 24_000;
/// The beep's peak, a twentieth of full scale.
pub(crate) const PEAK: f64 = 0.05;
/// How long each edge of a beep takes to rise or fall, in samples: 2 ms.
const EDGE_SAMPLES: u64 = 96;
/// The length of one buffer the stream hands over: web-audio-api's render quantum.
const QUANTUM: usize = 128;

/// A stream of one track recording `sound`.
pub(crate) fn stream(sound: SimulatedSound) -> MediaStream {
    let mut position = 0;
    let quanta = std::iter::repeat_with(move || {
        let samples = (0..QUANTUM)
            .map(|offset| sample_at(sound, position + offset as u64))
            .collect();
        position += QUANTUM as u64;
        Ok(AudioBuffer::from(vec![samples], SAMPLE_RATE))
    });
    MediaStream::from_tracks(vec![MediaStreamTrack::from_iter(quanta)])
}

/// The sample of `sound` at `position`, counted from the stream's start.
pub(crate) fn sample_at(sound: SimulatedSound, position: u64) -> f32 {
    match sound {
        SimulatedSound::Silence => 0.0,
        SimulatedSound::Tone => beep_at(position) as f32,
    }
}

/// The tone's sample at `position`: the sine, inside a beep, scaled by its edge; zero between.
fn beep_at(position: u64) -> f64 {
    let into_beep = position % PERIOD_SAMPLES;
    if into_beep >= BEEP_SAMPLES {
        return 0.0;
    }
    let edge = into_beep.min(BEEP_SAMPLES - 1 - into_beep);
    let envelope = if edge < EDGE_SAMPLES {
        let rise = edge as f64 / EDGE_SAMPLES as f64;
        (1.0 - (rise * std::f64::consts::PI).cos()) / 2.0
    } else {
        1.0
    };
    let time = position as f64 / f64::from(SAMPLE_RATE);
    PEAK * envelope * (TAU * TONE_HZ * time).sin()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tone's samples over `seconds`.
    fn tone(seconds: u64) -> Vec<f32> {
        let count = seconds * u64::from(SAMPLE_RATE as u32);
        (0..count)
            .map(|at| sample_at(SimulatedSound::Tone, at))
            .collect()
    }

    #[test]
    fn the_tone_beeps_twice_a_second_at_400_hz_never_above_a_twentieth_of_full_scale() {
        let samples = tone(2);
        assert!(samples.iter().all(|sample| sample.abs() <= PEAK as f32));
        let starts = samples
            .windows(2)
            .enumerate()
            .filter(|(at, pair)| {
                pair[0] == 0.0 && pair[1] != 0.0 && (*at as u64).is_multiple_of(PERIOD_SAMPLES)
            })
            .count();
        assert_eq!(starts, 4, "a beep every half second");
        let beep = &samples[..BEEP_SAMPLES as usize];
        let rising = beep
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        assert_eq!(rising, 8, "400 Hz is 8 periods in 20 ms, each rising once");
        let between = &samples[BEEP_SAMPLES as usize..PERIOD_SAMPLES as usize];
        assert!(
            between.iter().all(|sample| *sample == 0.0),
            "quiet between beeps"
        );
    }

    #[test]
    fn silence_is_zero_throughout() {
        assert!((0..PERIOD_SAMPLES).all(|at| sample_at(SimulatedSound::Silence, at) == 0.0));
    }

    #[test]
    fn the_stream_hands_its_samples_over_a_quantum_at_a_time_from_its_start() {
        let stream = stream(SimulatedSound::Tone);
        let mut track = stream.get_tracks()[0].iter();
        let first = track.next().unwrap().unwrap();
        let second = track.next().unwrap().unwrap();
        assert_eq!(first.length(), QUANTUM);
        let expected: Vec<f32> = (QUANTUM as u64..2 * QUANTUM as u64)
            .map(|at| sample_at(SimulatedSound::Tone, at))
            .collect();
        assert_eq!(second.get_channel_data(0), expected.as_slice());
    }
}
