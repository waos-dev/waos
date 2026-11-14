//! `wasdk:platform/audio`: a program's sound, Web Audio's graph as web-audio-api implements it,
//! served node for node over the contexts its environment's [`AudioDevices`] make (the sound
//! proposal's decisions 1 and 2). This module is the program's sound as a whole, [`ProgramAudio`]:
//! its devices, the outputs it may play on and the microphones it has open. A context is
//! `context`'s, an offline one `offline_context`'s, and what every kind of context is, with the
//! changes of its state, `base_context`'s; each node, as its kind's handle and as any node,
//! `nodes`', each kind's handle named in [`handles`]; a scheduled source, with its end,
//! `sources`'; the platform's own inputs `stream_input`'s and `microphone`'s; which outputs a
//! program may play on `outputs`'; what of its sound keeps it thawed, read at each tick of the
//! engine's time, `sound`'s; a file decoded, within what a buffer holds, `decoding`'s; the spec's
//! enumerations `enumerations`'; and how a mistake of the program's, which the crate reports by
//! panicking, ends it, `mistakes`'. The interface is served to a program by [`host`].
//!
//! The invariant: the graph renders on web-audio-api's own thread, and nothing of the program runs
//! there or waits for it: a program's call only queues what the render thread reads, so a frozen
//! program's scheduled sound plays out. Every call from a program into the crate that may panic
//! goes through `mistakes`, and every call that may wait on a device, or work long, decoding and
//! rendering offline, runs off the program's thread.

mod base_context;
mod context;
mod decoding;
mod enumerations;
pub mod handles;
pub(crate) mod host;
mod microphone;
mod mistakes;
mod nodes;
mod offline_context;
mod outputs;
mod simulated_microphone;
mod sound;
mod sources;
mod stream_input;

use std::future::Future;
use std::sync::Arc;

use waos_capabilities::web_audio_api::context::{
    AudioContextOptions, AudioContextState, BaseAudioContext,
};
use waos_capabilities::web_audio_api::media_streams::MediaStream;
use waos_capabilities::{Answer, AudioDeviceError, AudioDevices, ProgramGrants, Question};

use crate::bindings::wasdk::platform::audio::{AudioError, MediaDeviceInfo};
use crate::bindings::wasdk::platform::permissions::State;
use crate::engine_time::EngineClock;
use crate::lifecycle::Lifecycle;
use crate::permissions::{Permission, ProgramPermissions};
use crate::recordings::Recordings;
use crate::running::RunningProgram;
use crate::shell::ShellAudioOutputPicker;
pub use base_context::BaseContext;
pub use context::Context;
pub use microphone::Microphone;
use mistakes::failing;
pub use nodes::{AnyNode, Node};
pub use offline_context::OfflineContext;
use outputs::ProgramOutputs;
use sound::ContextSound;
pub use sources::Source;
pub use stream_input::StreamInput;

/// The value behind a program's `audio`: its sound, which the code serving it reaches through
/// [`ProgramAudio`].
pub struct Audio;

/// One program's sound: the devices its environment gives it, the outputs its user picked for
/// it, and the microphones it has open. Every clone is the same.
#[derive(Clone)]
pub(crate) struct ProgramAudio {
    devices: Option<Arc<dyn AudioDevices>>,
    lifecycle: Lifecycle,
    /// The engine's clock, at whose ticks each context's sound is read.
    engine_clock: EngineClock,
    outputs: ProgramOutputs,
    /// The microphones it has open, as the shell shows them.
    microphones: MicrophoneRecordings,
    /// Where its microphone answer is read, which picks the machine's microphone or one of WaOS's
    /// own, and what that one records.
    grants: Arc<dyn ProgramGrants>,
}

/// The microphones a program has open, as the shell's indicator shows them: the machine's, and
/// WaOS's own, shown apart.
#[derive(Clone)]
pub(crate) struct MicrophoneRecordings {
    pub(crate) machine: Recordings,
    pub(crate) simulated: Recordings,
}

impl ProgramAudio {
    /// `program`'s sound, on `devices`, its outputs picked in `output_picker`, where the shell has
    /// one, unless `grants` hold the picker's answer, and its microphones shown as `microphones`
    /// shows them; with no devices it has none.
    pub(crate) fn new(
        devices: Option<Arc<dyn AudioDevices>>,
        output_picker: Option<Arc<dyn ShellAudioOutputPicker>>,
        grants: Arc<dyn ProgramGrants>,
        microphones: MicrophoneRecordings,
        program: &RunningProgram,
    ) -> ProgramAudio {
        ProgramAudio {
            devices,
            lifecycle: program.lifecycle().clone(),
            engine_clock: program.engine_clock().clone(),
            outputs: ProgramOutputs::new(output_picker, Arc::clone(&grants), program.clone()),
            microphones,
            grants,
        }
    }

    /// Whether the program's microphone answer is simulated now.
    fn microphone_simulated(&self) -> bool {
        self.grants.answer(Question::Microphone) == Some(Answer::Simulated)
    }

    /// Whether the program has sound.
    pub(crate) fn exists(&self) -> bool {
        self.devices.is_some()
    }

    /// The devices of a program that has sound, as one holding an `audio` handle does.
    fn devices(&self) -> wasmtime::Result<Arc<dyn AudioDevices>> {
        self.devices
            .clone()
            .ok_or_else(|| wasmtime::format_err!("sound reached by a program given none"))
    }

    /// A context made as `options` say, on an output the program may play on.
    fn new_context(
        &self,
        options: AudioContextOptions,
    ) -> wasmtime::Result<Result<Context, AudioError>> {
        let devices = self.devices()?;
        if !self.outputs.allows(&options.sink_id) {
            return Ok(Err(AudioError::NotAllowed));
        }
        let sink_id = options.sink_id.clone();
        let made = device_answer(failing(|| devices.new_context(options)));
        Ok(made.map(|context| {
            let base = context.base().clone();
            let clock = move || base.current_time();
            let sound = ContextSound::new(self.lifecycle.clone(), &self.engine_clock, clock);
            Context::new(context, sink_id, sound)
        }))
    }

    /// Shows the user the shell's picker of outputs, and answers the one picked.
    async fn select_output(&self) -> wasmtime::Result<Result<MediaDeviceInfo, AudioError>> {
        let devices = self.devices()?;
        let outputs = off_the_program_thread(move || devices.outputs()).await?;
        Ok(self.outputs.select(outputs).await)
    }

    /// Moves `context` onto the output `sink_id` names, if the program may play on it.
    async fn set_sink_id(
        &self,
        context: Context,
        sink_id: String,
    ) -> wasmtime::Result<Result<(), AudioError>> {
        if !self.outputs.allows(&sink_id) {
            return Ok(Err(AudioError::NotAllowed));
        }
        if context.context().state() == AudioContextState::Closed {
            return Ok(Err(closed()));
        }
        let devices = self.devices()?;
        let (shared, named) = (context.shared(), sink_id.clone());
        let moved =
            off_the_program_thread(move || failing(|| devices.set_output(&shared, &named))).await?;
        Ok(device_answer(moved).map(|()| context.set_sink_id(sink_id)))
    }

    /// The microphone permission, asked of the user at its first use, where the devices have a
    /// microphone and the shell can show that the program records: where it stands once the user
    /// is done. Nothing is asked otherwise.
    fn microphone_question(
        &self,
        permissions: &ProgramPermissions,
    ) -> wasmtime::Result<Result<impl Future<Output = State> + Send + 'static + use<>, AudioError>>
    {
        if !self.microphone_simulated() && !self.devices()?.has_microphone() {
            return Ok(Err(AudioError::NotSupported(
                "this platform gives the program no microphone".to_owned(),
            )));
        }
        if !self.microphones.machine.can_show() {
            return Ok(Err(AudioError::NotSupported(
                "this platform cannot show that a program records".to_owned(),
            )));
        }
        Ok(Ok(
            permissions.request(Permission::Question(Question::Microphone))
        ))
    }

    /// The microphone, open as a source of `context`, once `asked`, the microphone question,
    /// says the user allows it.
    async fn open_microphone(
        &self,
        context: Context,
        asked: impl Future<Output = State>,
    ) -> wasmtime::Result<Result<Microphone, AudioError>> {
        if asked.await != State::Granted {
            return Ok(Err(AudioError::NotAllowed));
        }
        let simulated = self.microphone_simulated();
        let stream = if simulated {
            simulated_microphone::stream(self.grants.simulated_microphone())
        } else {
            match self.machine_microphone().await? {
                Ok(stream) => stream,
                Err(error) => return Ok(Err(error)),
            }
        };
        // web-audio-api panics at a stream with no track, which only a provider gives.
        let source = failing(|| context.context().create_media_stream_source(&stream));
        Ok(source
            .map(|source| {
                let recordings = if simulated {
                    &self.microphones.simulated
                } else {
                    &self.microphones.machine
                };
                let recording = recordings.record();
                Microphone::new(source, stream, recording, self.lifecycle.sounding())
            })
            .map_err(AudioError::InvalidState))
    }
}

impl ProgramAudio {
    /// The machine's default microphone, open, from the devices off the program's thread, since
    /// opening it may wait on the device.
    async fn machine_microphone(&self) -> wasmtime::Result<Result<MediaStream, AudioError>> {
        let devices = self.devices()?;
        let opened = off_the_program_thread(move || failing(|| devices.open_microphone())).await?;
        Ok(device_answer(opened))
    }
}

/// `work`, which may wait on a device or take long, on a thread of the runtime's blocking pool,
/// so the program's other tasks go on meanwhile.
async fn off_the_program_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> wasmtime::Result<T> {
    tokio::task::spawn_blocking(work).await.map_err(|error| {
        wasmtime::format_err!("a sound device's call ended without an answer: {error}")
    })
}

/// What the program is answered for a device's `answer`: its value, its error, or the panic
/// web-audio-api answered with, as a device that failed.
fn device_answer<T>(answer: Result<Result<T, AudioDeviceError>, String>) -> Result<T, AudioError> {
    match answer {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(AudioDeviceError::NotFound)) => Err(AudioError::NotFound),
        Ok(Err(AudioDeviceError::NotSupported(why))) => Err(AudioError::NotSupported(why)),
        Ok(Err(AudioDeviceError::Failed(why))) | Err(why) => Err(AudioError::InvalidState(why)),
    }
}

/// The most samples a buffer may hold: as many as a program's memory could, four bytes each.
pub(crate) const MOST_SAMPLES: u64 = 1 << 30;

/// What a closed context answers.
fn closed() -> AudioError {
    AudioError::InvalidState("the context is closed".to_owned())
}
