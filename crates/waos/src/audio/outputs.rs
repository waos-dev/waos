//! The outputs a program may play on: the system's default, and each one its user picked for it in
//! the shell's picker, for the rest of its run, as the web's `selectAudioOutput` grants one, or set
//! as the picker's answer among its settings, which is the pick without showing anything; an
//! output set that is not among the outputs now is `not-found`, never another. A program never
//! sees the list of outputs, only the one picked.
//!
//! The invariant: an output is the program's only once the user picked it for this program; a
//! context made or moved onto any other is refused.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use waos_capabilities::AudioOutput;

use crate::access::ProgramAccess;
use crate::bindings::wasdk::platform::audio::{AudioError, MediaDeviceInfo};
use crate::running::RunningProgram;
use crate::shell::ShellAudioOutputPicker;
use crate::sync::locked;

/// What a program's contexts may play on. Every clone is the same.
#[derive(Clone)]
pub(super) struct ProgramOutputs(Arc<OutputsParts>);

struct OutputsParts {
    picker: Option<Arc<dyn ShellAudioOutputPicker>>,
    /// Where the output the user set as the picker's answer is read.
    access: ProgramAccess,
    program: RunningProgram,
    /// The ids of the outputs the user picked for the program.
    picked: Mutex<HashSet<String>>,
}

impl ProgramOutputs {
    /// `program`'s, picked through `picker` unless the grants `access` reads hold an answer set
    /// for it; with neither, only the default.
    pub(super) fn new(
        picker: Option<Arc<dyn ShellAudioOutputPicker>>,
        access: ProgramAccess,
        program: RunningProgram,
    ) -> ProgramOutputs {
        ProgramOutputs(Arc::new(OutputsParts {
            picker,
            access,
            program,
            picked: Mutex::default(),
        }))
    }

    /// Whether the program may play on `sink_id`: the default's empty string, or an output its
    /// user picked for it.
    pub(super) fn allows(&self, sink_id: &str) -> bool {
        sink_id.is_empty() || locked(&self.0.picked).contains(sink_id)
    }

    /// Answers the output among `outputs` the user set as the picker's answer, or else shows the
    /// user `outputs` in the shell's picker and answers the one picked: the program may play on it
    /// from then on.
    pub(super) async fn select(
        &self,
        outputs: Vec<AudioOutput>,
    ) -> Result<MediaDeviceInfo, AudioError> {
        let output = match self.0.access.grants().audio_output_answer() {
            Some(set) => self.set_answer(set, outputs)?,
            None => self.picked(outputs).await?,
        };
        locked(&self.0.picked).insert(output.id.clone());
        Ok(MediaDeviceInfo {
            device_id: output.id,
            group_id: output.group_id.unwrap_or_default(),
            label: output.label,
        })
    }

    /// The output `set` names among `outputs`; `not-found` when it is not among them.
    fn set_answer(
        &self,
        set: String,
        outputs: Vec<AudioOutput>,
    ) -> Result<AudioOutput, AudioError> {
        let found = outputs.into_iter().find(|output| output.id == set);
        found.ok_or_else(|| {
            let program = self.0.program.name();
            tracing::warn!(
                program,
                output = set,
                "the output set as the picker's answer is not connected"
            );
            AudioError::NotFound
        })
    }

    /// The output the user picks among `outputs` in the shell's picker.
    async fn picked(&self, outputs: Vec<AudioOutput>) -> Result<AudioOutput, AudioError> {
        let Some(picker) = &self.0.picker else {
            return Err(AudioError::NotSupported(
                "this platform has no picker of outputs".to_owned(),
            ));
        };
        if outputs.is_empty() {
            return Err(AudioError::NotFound);
        }
        let picked = picker.pick(&self.0.program, outputs.clone()).await;
        picked
            .and_then(|id| outputs.into_iter().find(|output| output.id == id))
            .ok_or(AudioError::NotAllowed)
    }
}
