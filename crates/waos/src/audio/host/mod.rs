//! The code serving `wasdk:platform/audio` to a program: the platform's sound and its picker of
//! outputs and a context's options, here; each kind of context and what every context is, its
//! clock, its state and its factories, in `contexts`; what every node is and what the graph's values are, params, buffers, periodic
//! waves and the listener, in `graph`; each kind of node in `kinds`; and the platform's own
//! inputs in `inputs`.
//!
//! A handle is pushed into the store's table over the audio module's value for it, or
//! web-audio-api's own where the crate has one; a context's handle and each base it hands out hold
//! the same context, and a node's kind handle and each `audio-node` it hands out the same node.

mod contexts;
mod graph;
mod inputs;
mod kinds;

use waos_capabilities::web_audio_api::context::{
    AudioContextLatencyCategory as LatencyCategory, AudioContextOptions,
};
use wasmtime::component::{Accessor, HasData, Linker, Resource, ResourceTable};

use super::{Audio, Context, ProgramAudio};
use crate::bindings::wasdk::platform::audio::{
    self, AudioContextLatencyCategory, AudioError, HostAudio, HostAudioContextOptions,
    HostAudioWithStore, MediaDeviceInfo,
};
use crate::permissions::ProgramPermissions;

/// What the audio interface reads and writes in a program's store.
pub(crate) struct AudioCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) audio: &'a ProgramAudio,
    /// What asks the user for the microphone.
    pub(crate) permissions: &'a ProgramPermissions,
}

pub(crate) trait AudioCtxView: Send {
    fn audio_ctx(&mut self) -> AudioCtx<'_>;
}

struct HasAudio;

impl HasData for HasAudio {
    type Data<'a> = AudioCtx<'a>;
}

pub(crate) fn add_to_linker<T: AudioCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    audio::add_to_linker::<_, HasAudio>(linker, T::audio_ctx)
}

impl AudioCtx<'_> {
    /// `value` as a new handle.
    fn handle<T: Send + 'static>(&mut self, value: T) -> wasmtime::Result<Resource<T>> {
        Ok(self.table.push(value)?)
    }
}

impl audio::Host for AudioCtx<'_> {
    fn get_audio(&mut self) -> wasmtime::Result<Option<Resource<Audio>>> {
        if !self.audio.exists() {
            return Ok(None);
        }
        Ok(Some(self.handle(Audio)?))
    }
}

impl HostAudio for AudioCtx<'_> {
    fn new_context(
        &mut self,
        _audio: Resource<Audio>,
        options: Resource<AudioContextOptions>,
    ) -> wasmtime::Result<Result<Resource<Context>, AudioError>> {
        let options = self.table.delete(options)?;
        match self.audio.new_context(options)? {
            Ok(context) => Ok(Ok(self.handle(context)?)),
            Err(error) => Ok(Err(error)),
        }
    }

    fn drop(&mut self, audio: Resource<Audio>) -> wasmtime::Result<()> {
        self.table.delete(audio)?;
        Ok(())
    }
}

impl<T: AudioCtxView + 'static> HostAudioWithStore<T> for HasAudio {
    async fn select_audio_output(
        accessor: &Accessor<T, Self>,
        _audio: Resource<Audio>,
    ) -> wasmtime::Result<Result<MediaDeviceInfo, AudioError>> {
        let audio = accessor.with(|mut access| access.get().audio.clone());
        audio.select_output().await
    }
}

impl HostAudioContextOptions for AudioCtx<'_> {
    fn new(&mut self) -> wasmtime::Result<Resource<AudioContextOptions>> {
        self.handle(AudioContextOptions::default())
    }

    fn latency_hint(
        &mut self,
        options: Resource<AudioContextOptions>,
        category: AudioContextLatencyCategory,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.latency_hint = category.into();
        Ok(())
    }

    fn latency_hint_seconds(
        &mut self,
        options: Resource<AudioContextOptions>,
        seconds: f64,
    ) -> wasmtime::Result<()> {
        wasmtime::ensure!(
            seconds > 0.0,
            "RangeError - a latency of {seconds} s: it must be more than none"
        );
        self.table.get_mut(&options)?.latency_hint = LatencyCategory::Custom(seconds);
        Ok(())
    }

    fn sample_rate(
        &mut self,
        options: Resource<AudioContextOptions>,
        sample_rate: f32,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.sample_rate = Some(sample_rate);
        Ok(())
    }

    fn sink_id(
        &mut self,
        options: Resource<AudioContextOptions>,
        sink_id: String,
    ) -> wasmtime::Result<()> {
        self.table.get_mut(&options)?.sink_id = sink_id;
        Ok(())
    }

    fn drop(&mut self, options: Resource<AudioContextOptions>) -> wasmtime::Result<()> {
        self.table.delete(options)?;
        Ok(())
    }
}
