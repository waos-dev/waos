//! This computer's sound devices as a capability: [`LocalAudio`], the provider of
//! `waos-capabilities`' [`AudioDevices`] for the machine in front of the user. Its contexts play
//! on the system's audio and its microphone records from it, as web-audio-api reaches them
//! through cpal: Core Audio on Apple's systems, WASAPI on Windows, ALSA on Linux, AAudio on
//! Android.
//!
//! The invariant: this crate depends on `waos-capabilities` only. It names no shell, no engine and
//! no program's internals; a program gets it when its environment is chosen, at start. What
//! web-audio-api reports by panicking, opening the microphone, it answers as an error. Every call
//! that lists the system's devices or opens one runs where the system takes it (`calling_stack`):
//! on Android, off the stack the engine calls on.

mod calling_stack;

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};

use waos_capabilities::web_audio_api::context::{AudioContext, AudioContextOptions};
use waos_capabilities::web_audio_api::media_devices::{
    self, MediaDeviceInfo, MediaDeviceInfoKind, MediaStreamConstraints,
};
use waos_capabilities::web_audio_api::media_streams::MediaStream;
use waos_capabilities::{AudioDeviceError, AudioDevices, AudioOutput};

use crate::calling_stack::on_a_threads_own;

/// The sample rates a context may run at: Web Audio's range.
const SAMPLE_RATES: std::ops::RangeInclusive<f32> = 3_000.0..=768_000.0;

/// This computer's outputs and microphone, through the system's audio; without the microphone
/// where the app may not record sound.
#[derive(Clone, Copy, Debug)]
pub struct LocalAudio {
    microphone: bool,
}

impl LocalAudio {
    /// This computer's outputs and microphone.
    pub fn new() -> LocalAudio {
        LocalAudio { microphone: true }
    }

    /// This computer's outputs alone, for an app that may not record sound, as one whose bundle
    /// does not say why it does.
    pub fn without_microphone() -> LocalAudio {
        LocalAudio { microphone: false }
    }
}

impl Default for LocalAudio {
    fn default() -> LocalAudio {
        LocalAudio::new()
    }
}

impl AudioDevices for LocalAudio {
    fn new_context(&self, options: AudioContextOptions) -> Result<AudioContext, AudioDeviceError> {
        if let Some(rate) = options.sample_rate
            && !SAMPLE_RATES.contains(&rate)
        {
            return Err(AudioDeviceError::NotSupported(format!(
                "a sample rate of {rate} Hz"
            )));
        }
        on_a_threads_own(|| {
            check_output(&options.sink_id)?;
            AudioContext::try_new(options)
                .map_err(|error| AudioDeviceError::Failed(error.to_string()))
        })
    }

    fn outputs(&self) -> Vec<AudioOutput> {
        on_a_threads_own(outputs)
    }

    fn set_output(&self, context: &AudioContext, sink_id: &str) -> Result<(), AudioDeviceError> {
        on_a_threads_own(|| {
            check_output(sink_id)?;
            context
                .set_sink_id_sync(sink_id.to_owned())
                .map_err(|error| AudioDeviceError::Failed(error.to_string()))
        })
    }

    fn has_microphone(&self) -> bool {
        self.microphone
    }

    fn open_microphone(&self) -> Result<MediaStream, AudioDeviceError> {
        if !self.microphone {
            return Err(AudioDeviceError::NotSupported(
                "this app may not record sound".to_owned(),
            ));
        }
        on_a_threads_own(open_default_microphone)
    }
}

/// The system's outputs, in the order it lists them.
fn outputs() -> Vec<AudioOutput> {
    devices(MediaDeviceInfoKind::AudioOutput)
        .map(|device| AudioOutput {
            id: device.device_id().to_owned(),
            group_id: device.group_id().map(str::to_owned),
            label: device.label().to_owned(),
        })
        .collect()
}

/// Whether `sink_id` names an output: the default's empty string, or one of the outputs.
fn check_output(sink_id: &str) -> Result<(), AudioDeviceError> {
    if sink_id.is_empty() || outputs().iter().any(|output| output.id == sink_id) {
        Ok(())
    } else {
        Err(AudioDeviceError::NotFound)
    }
}

/// The system's default microphone, open; `NotFound` where the system lists no input.
fn open_default_microphone() -> Result<MediaStream, AudioDeviceError> {
    if devices(MediaDeviceInfoKind::AudioInput).next().is_none() {
        return Err(AudioDeviceError::NotFound);
    }
    // web-audio-api panics when the system cannot open the input it listed.
    panic::catch_unwind(AssertUnwindSafe(|| {
        media_devices::get_user_media_sync(MediaStreamConstraints::Audio)
    }))
    .map_err(|panic| AudioDeviceError::Failed(message_of(panic.as_ref())))
}

/// The system's devices of `kind`.
fn devices(kind: MediaDeviceInfoKind) -> impl Iterator<Item = MediaDeviceInfo> {
    media_devices::enumerate_devices_sync()
        .into_iter()
        .filter(move |device| device.kind() == kind)
}

/// What a panic said.
fn message_of(panic: &(dyn Any + Send)) -> String {
    match panic.downcast_ref::<&str>() {
        Some(message) => (*message).to_owned(),
        None => panic
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "the microphone failed to open".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_rate_out_of_web_audio_s_range_is_not_supported() {
        let options = AudioContextOptions {
            sample_rate: Some(1_000.0),
            ..AudioContextOptions::default()
        };
        assert!(matches!(
            LocalAudio::new().new_context(options),
            Err(AudioDeviceError::NotSupported(_))
        ));
    }

    #[test]
    fn an_output_the_system_does_not_list_is_not_found() {
        let options = AudioContextOptions {
            sink_id: "no such output".to_owned(),
            ..AudioContextOptions::default()
        };
        assert!(matches!(
            LocalAudio::new().new_context(options),
            Err(AudioDeviceError::NotFound)
        ));
    }

    #[test]
    fn devices_without_the_microphone_say_so_and_open_none() {
        let audio = LocalAudio::without_microphone();
        assert!(!audio.has_microphone());
        assert!(matches!(
            audio.open_microphone(),
            Err(AudioDeviceError::NotSupported(_))
        ));
        assert!(LocalAudio::new().has_microphone());
    }
}
