//! A program's sound devices: what its Web Audio graph plays on and records from.
//!
//! The graph itself is `web_audio_api`'s, Web Audio implemented whole, which the engine serves to
//! the program node for node (the sound proposal's decision 1); the crate is re-exported, as
//! valo's font manager is, so a provider names the version the engine serves. What a provider
//! decides is which devices a context plays on and the microphone records from, this machine's
//! through the system's audio, or none, as a test's are; and whether there is a microphone to give
//! at all.
//!
//! The invariant: a provider makes and moves contexts and opens the microphone; it keeps no graph
//! and asks the user nothing. Which outputs a program may use is the engine's to decide, from what
//! the user picked.

use std::fmt;

use web_audio_api::context::{AudioContext, AudioContextOptions};
use web_audio_api::media_streams::MediaStream;

/// The sound devices a program's graph reaches: a context made on an output, the outputs the user
/// picks among, and the microphone. Called from the program's thread.
pub trait AudioDevices: Send + Sync {
    /// A context made as `options` say, playing on the output its `sink_id` names: one of
    /// [`outputs`](Self::outputs), or the empty string for the system's default.
    fn new_context(&self, options: AudioContextOptions) -> Result<AudioContext, AudioDeviceError>;

    /// The outputs the user can pick from, in the order a picker shows them.
    fn outputs(&self) -> Vec<AudioOutput>;

    /// Moves `context` to the output `sink_id` names, as [`new_context`](Self::new_context)
    /// reads it, keeping its graph and its state. May wait for the device.
    fn set_output(&self, context: &AudioContext, sink_id: &str) -> Result<(), AudioDeviceError>;

    /// Whether these devices have a microphone the program may be given: false where the app may
    /// not record sound, as an app whose bundle does not say why it does. A microphone that is not
    /// attached now still counts: [`open_microphone`](Self::open_microphone) answers `NotFound`
    /// for it.
    fn has_microphone(&self) -> bool;

    /// The system's default microphone, open: a stream of one track, which records until the
    /// track is closed.
    fn open_microphone(&self) -> Result<MediaStream, AudioDeviceError>;
}

/// An output of the machine's, as the user picks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioOutput {
    /// What names it to [`AudioDevices`]: a context's `sink_id`.
    pub id: String,
    /// Shared by the outputs of one physical device, where the system says.
    pub group_id: Option<String>,
    /// Its name, as the user knows it.
    pub label: String,
}

/// Why a device was not opened, or a context not made on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioDeviceError {
    /// No such device, or none at all.
    NotFound,
    /// What the device or the provider cannot do, such as a sample rate out of its range.
    NotSupported(String),
    /// The device failed.
    Failed(String),
}

impl fmt::Display for AudioDeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioDeviceError::NotFound => f.write_str("no such device"),
            AudioDeviceError::NotSupported(why) => write!(f, "not supported: {why}"),
            AudioDeviceError::Failed(why) => write!(f, "the device failed: {why}"),
        }
    }
}

impl std::error::Error for AudioDeviceError {}
