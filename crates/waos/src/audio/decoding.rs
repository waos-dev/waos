//! A program's sound file decoded into a buffer at its context's sample rate, as web-audio-api
//! decodes one, and capped as a buffer's size is: a small file may decode to more samples than a
//! program's memory could hold, by a codec that packs much into little or by a sample rate far
//! below the context's, and web-audio-api would then exhaust the machine and abort all of WaOS.
//! So the file is measured first, decoded once by symphonia, as web-audio-api decodes it, keeping
//! no samples, and refused once what decoding it holds passes the cap; only a file within it is
//! decoded by web-audio-api.
//!
//! The invariant: the measure decodes as web-audio-api's decoding does, the same probe of an
//! unseekable stream, the same track, the same packets kept and skipped, so it never counts fewer
//! frames than web-audio-api then decodes; and it counts what decoding holds at its most, the
//! frames decoded or the frames resampled to the context's rate, whichever are more.

use std::io::Cursor;
use std::sync::Arc;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;
use waos_capabilities::web_audio_api::AudioBuffer;
use waos_capabilities::web_audio_api::context::{BaseAudioContext, ConcreteBaseAudioContext};

use super::MOST_SAMPLES;
use super::mistakes::failing;
use crate::bindings::wasdk::platform::audio::AudioError;

/// `data`, a sound file, decoded into a buffer at `context`'s sample rate; refused when it would
/// hold more samples than a buffer may.
pub(super) fn decoded(
    data: Vec<u8>,
    context: &ConcreteBaseAudioContext,
) -> Result<AudioBuffer, AudioError> {
    let data = FileData(Arc::new(data));
    match fits(&data, context.sample_rate()) {
        Ok(true) => {}
        Ok(false) => return Err(too_long()),
        Err(error) => return Err(AudioError::Encoding(error.to_string())),
    }
    match failing(|| context.decode_audio_data_sync(Cursor::new(data))) {
        Ok(Ok(buffer)) => Ok(buffer),
        Ok(Err(error)) => Err(AudioError::Encoding(error.to_string())),
        Err(panic) => Err(AudioError::Encoding(panic)),
    }
}

/// What a file too long to decode answers.
fn too_long() -> AudioError {
    AudioError::NotSupported(format!(
        "the sound decodes to more than {MOST_SAMPLES} samples, which a buffer holds at most"
    ))
}

/// The bytes of a file, shared by the measure and web-audio-api's decoding, read where each
/// reads them.
#[derive(Clone)]
struct FileData(Arc<Vec<u8>>);

impl AsRef<[u8]> for FileData {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Whether `data` decodes, at a context's `sample_rate`, to no more samples than a buffer may
/// hold: decoded as web-audio-api decodes it, counting the frames, and answered as soon as they
/// pass the cap.
fn fits(data: &FileData, sample_rate: f32) -> Result<bool, SymphoniaError> {
    let source = ReadOnlySource::new(Cursor::new(data.clone()));
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let mut format = symphonia::default::get_probe().probe(
        &Hint::new(),
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or(SymphoniaError::Unsupported(
            "no default media track available",
        ))?;
    let track_id = track.id;
    let codec = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or(SymphoniaError::Unsupported(
            "the default track is not audio",
        ))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec, &AudioDecoderOptions::default().verify(true))?;
    let mut frames = 0;
    while let Some(packet) = packet_or_end(format.next_packet())? {
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                frames += decoded.frames() as u64;
                let spec = decoded.spec();
                let held = samples_held(frames, spec.channels().count(), spec.rate(), sample_rate);
                if held > MOST_SAMPLES {
                    return Ok(false);
                }
            }
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

/// The packet `read`, none at the file's end, which a file cut short reaches too.
fn packet_or_end<P>(read: Result<Option<P>, SymphoniaError>) -> Result<Option<P>, SymphoniaError> {
    match read {
        Err(SymphoniaError::IoError(error))
            if error.kind() == std::io::ErrorKind::UnexpectedEof =>
        {
            Ok(None)
        }
        read => read,
    }
}

/// The samples decoding `frames` frames of `channels` channels at `rate` holds at its most, at a
/// context's `sample_rate`: the frames decoded, or the frames resampled, whichever are more.
fn samples_held(frames: u64, channels: usize, rate: u32, sample_rate: f32) -> u64 {
    let resampled = (frames as f64 * f64::from(sample_rate) / f64::from(rate)).ceil();
    // A rate of nothing resamples to more than anything holds, which `as` saturates to.
    let most = frames.max(resampled as u64);
    most.saturating_mul(channels as u64)
}

#[cfg(test)]
mod tests {
    use waos_capabilities::web_audio_api::context::OfflineAudioContext;

    use super::*;

    /// A WAV file of `frames` frames of 16-bit samples of `value`, one channel at `rate`.
    fn wav(rate: u32, frames: usize, value: i16) -> Vec<u8> {
        let data_size = (frames * 2) as u32;
        let mut file = Vec::with_capacity(44 + frames * 2);
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
        for _ in 0..frames {
            file.extend_from_slice(&value.to_le_bytes());
        }
        file
    }

    /// A context at 48 kHz, which needs no device.
    fn context() -> ConcreteBaseAudioContext {
        OfflineAudioContext::new(1, 128, 48_000.0).base().clone()
    }

    #[test]
    fn a_file_within_the_cap_decodes_as_web_audio_api_decodes_it() {
        let buffer = decoded(wav(24_000, 64, 8192), &context()).unwrap();
        assert_eq!(buffer.sample_rate(), 48_000.0);
        assert_eq!(buffer.length(), 128, "resampled to the context's rate");
        assert_eq!(buffer.get_channel_data(0)[0], 0.25);
    }

    /// 45 kB claiming a rate of one frame a second: six hours, which at 48 kHz would be 4 GiB of
    /// samples.
    #[test]
    fn a_small_file_claiming_hours_is_refused_before_it_is_decoded() {
        let file = wav(1, 22_400, 0);
        assert!(file.len() < 45_000);
        let refused = decoded(file, &context()).expect_err("too long to decode");
        let AudioError::NotSupported(why) = refused else {
            panic!("refused as more than the platform holds: {refused:?}");
        };
        assert!(why.contains("1073741824 samples"), "{why}");
    }

    #[test]
    fn a_file_at_a_frame_a_second_within_the_cap_decodes() {
        let buffer = decoded(wav(1, 10, 0), &context()).unwrap();
        assert_eq!(buffer.length(), 480_000, "resampled to 48 kHz");
    }

    #[test]
    fn what_is_not_sound_is_refused_as_such() {
        let refused = decoded(b"no sound here".to_vec(), &context()).unwrap_err();
        assert!(matches!(refused, AudioError::Encoding(_)), "{refused:?}");
    }

    #[test]
    fn decoding_holds_the_frames_decoded_or_resampled_whichever_are_more() {
        assert_eq!(samples_held(100, 2, 48_000, 48_000.0), 200);
        assert_eq!(samples_held(100, 2, 24_000, 48_000.0), 400, "upsampled");
        assert_eq!(samples_held(100, 2, 96_000, 48_000.0), 200, "downsampled");
        assert_eq!(samples_held(3, 1, 2, 3.0), 5, "a part frame is a frame");
        assert_eq!(samples_held(1, 1, 0, 48_000.0), u64::MAX, "no rate");
    }
}
