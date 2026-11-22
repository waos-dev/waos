//! The machine's sound as the tests have it: devices of the test's own, whose contexts play on no
//! output, so no test opens the machine's, with two outputs to pick from and a microphone that
//! hears a constant, or none where a test says; and the shell's picker, which picks what the test said, and its indicator of
//! who records, and with what, which keeps what it was told.

use std::sync::Mutex;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::ready;
use waos::{
    AudioDeviceError, AudioDevices, AudioOutput, AudioOutputPickedFuture, RecordingDevice,
    RunningProgram, ShellAudioOutputPicker, ShellRecordingIndicator,
};
use waos_capabilities::web_audio_api::AudioBuffer;
use waos_capabilities::web_audio_api::context::{AudioContext, AudioContextOptions};
use waos_capabilities::web_audio_api::media_streams::{MediaStream, MediaStreamTrack};

/// What the test microphone hears, always.
pub const HEARD: f32 = 0.25;

/// The id of the output the picker picks unless a test says otherwise.
pub const HEADPHONES: &str = "headphones";

/// Sound devices of the test's own.
#[derive(Default)]
pub struct TestDevices {
    /// The output each context was moved onto, in order.
    moves: Mutex<Vec<String>>,
    /// Whether they have no microphone, as an app's that may not record sound.
    without_microphone: bool,
}

impl TestDevices {
    /// Devices with no microphone, as an app's that may not record sound.
    pub fn without_microphone() -> TestDevices {
        TestDevices {
            without_microphone: true,
            ..TestDevices::default()
        }
    }

    /// The output each context was moved onto, in order.
    pub fn moves(&self) -> Vec<String> {
        self.moves.lock().unwrap().clone()
    }
}

impl AudioDevices for TestDevices {
    /// A context playing on no output, whichever the program named, rendering as one on a
    /// device would, in time.
    fn new_context(&self, options: AudioContextOptions) -> Result<AudioContext, AudioDeviceError> {
        let options = AudioContextOptions {
            sink_id: "none".to_owned(),
            ..options
        };
        AudioContext::try_new(options).map_err(|error| AudioDeviceError::Failed(error.to_string()))
    }

    fn outputs(&self) -> Vec<AudioOutput> {
        vec![
            AudioOutput {
                id: "speakers".to_owned(),
                group_id: None,
                label: "Speakers".to_owned(),
            },
            AudioOutput {
                id: HEADPHONES.to_owned(),
                group_id: Some("jack".to_owned()),
                label: "Headphones".to_owned(),
            },
        ]
    }

    fn set_output(&self, _context: &AudioContext, sink_id: &str) -> Result<(), AudioDeviceError> {
        self.moves.lock().unwrap().push(sink_id.to_owned());
        Ok(())
    }

    fn has_microphone(&self) -> bool {
        !self.without_microphone
    }

    /// A microphone that hears [`HEARD`] for as long as it is open.
    fn open_microphone(&self) -> Result<MediaStream, AudioDeviceError> {
        let block = AudioBuffer::from(vec![vec![HEARD; 128]], 48_000.0);
        let track = MediaStreamTrack::from_iter(std::iter::repeat_with(move || Ok(block.clone())));
        Ok(MediaStream::from_tracks(vec![track]))
    }
}

/// The shell's sound: the picker and the indicator of who records.
pub struct TestSound {
    /// The output the user picks; none to dismiss the picker.
    pick: Mutex<Option<String>>,
    /// When true, holds picker requests waiting until dropped.
    hold_pick: Mutex<bool>,
    held: Mutex<Vec<oneshot::Sender<Option<String>>>>,
    /// The outputs each picker showed, in order.
    shown: Mutex<Vec<Vec<String>>>,
    /// What the indicator was told, in order: which program, with what, and whether it records.
    recording: Mutex<Vec<(String, RecordingDevice, bool)>>,
}

impl Default for TestSound {
    fn default() -> TestSound {
        TestSound {
            pick: Mutex::new(Some(HEADPHONES.to_owned())),
            hold_pick: Mutex::default(),
            held: Mutex::default(),
            shown: Mutex::default(),
            recording: Mutex::default(),
        }
    }
}

impl TestSound {
    /// The user picks `output` in each picker from now on; none dismisses it.
    pub fn set_pick(&self, output: Option<&str>) {
        *self.pick.lock().unwrap() = output.map(str::to_owned);
    }

    /// Holds picker requests from now on until their futures are dropped.
    pub fn hold_picks(&self) {
        *self.hold_pick.lock().unwrap() = true;
    }

    /// Whether the `index`th held picker's future was dropped without answering it.
    pub fn held_cancelled(&self, index: usize) -> bool {
        self.held
            .lock()
            .unwrap()
            .get(index)
            .is_some_and(oneshot::Sender::is_canceled)
    }

    /// The outputs each picker showed, by their ids, in order.
    pub fn shown(&self) -> Vec<Vec<String>> {
        self.shown.lock().unwrap().clone()
    }

    /// What the indicator was told, in order.
    pub fn recording(&self) -> Vec<(String, RecordingDevice, bool)> {
        self.recording.lock().unwrap().clone()
    }
}

impl ShellAudioOutputPicker for TestSound {
    fn pick(
        &self,
        _program: &RunningProgram,
        outputs: Vec<AudioOutput>,
    ) -> AudioOutputPickedFuture {
        let ids = outputs.into_iter().map(|output| output.id).collect();
        self.shown.lock().unwrap().push(ids);
        if *self.hold_pick.lock().unwrap() {
            let (tx, rx) = oneshot::channel();
            self.held.lock().unwrap().push(tx);
            return Box::pin(rx.map(|picked| picked.unwrap_or(None)));
        }
        Box::pin(ready(self.pick.lock().unwrap().clone()))
    }
}

impl ShellRecordingIndicator for TestSound {
    fn recording_changed(
        &self,
        program: &RunningProgram,
        device: RecordingDevice,
        recording: bool,
    ) {
        self.recording
            .lock()
            .unwrap()
            .push((program.name().to_owned(), device, recording));
    }
}
