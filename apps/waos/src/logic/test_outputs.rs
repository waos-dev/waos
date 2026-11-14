//! Sound devices of the tests' own, with outputs the test states, in place of this computer's,
//! so a test reads no device of the machine it runs on: no context is made, no microphone opens.

use waos::{AudioDeviceError, AudioDevices, AudioOutput};
use waos_capabilities::web_audio_api::context::{AudioContext, AudioContextOptions};
use waos_capabilities::web_audio_api::media_streams::MediaStream;

/// The outputs a picker offers in the tests.
pub(crate) const STATED_OUTPUTS: [(&str, &str); 2] =
    [("speakers", "Speakers"), ("headphones", "Headphones")];

/// Devices that offer [`STATED_OUTPUTS`] and play on nothing.
pub(crate) struct StatedOutputs;

impl AudioDevices for StatedOutputs {
    fn new_context(&self, _options: AudioContextOptions) -> Result<AudioContext, AudioDeviceError> {
        Err(AudioDeviceError::NotSupported(
            "the tests' devices play nothing".to_owned(),
        ))
    }

    fn outputs(&self) -> Vec<AudioOutput> {
        STATED_OUTPUTS
            .iter()
            .map(|(id, label)| AudioOutput {
                id: (*id).to_owned(),
                group_id: None,
                label: (*label).to_owned(),
            })
            .collect()
    }

    fn set_output(&self, _context: &AudioContext, _sink_id: &str) -> Result<(), AudioDeviceError> {
        Err(AudioDeviceError::NotFound)
    }

    fn has_microphone(&self) -> bool {
        false
    }

    fn open_microphone(&self) -> Result<MediaStream, AudioDeviceError> {
        Err(AudioDeviceError::NotFound)
    }
}
