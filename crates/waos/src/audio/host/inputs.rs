//! The platform's own inputs: a stream input, which the program writes ahead of its graph, and the
//! microphone.

use wasmtime::component::{Accessor, Resource};

use super::{AudioCtx, AudioCtxView, HasAudio};
use crate::audio::stream_input::WriteError;
use crate::audio::{AnyNode, Microphone, StreamInput};
use crate::bindings::wasdk::platform::audio::{
    AudioError, HostMicrophoneNode, HostStreamInputNode, HostStreamInputNodeWithStore,
};

impl HostStreamInputNode for AudioCtx<'_> {
    fn node(&mut self, input: Resource<StreamInput>) -> wasmtime::Result<Resource<AnyNode>> {
        let any = self.table.get(&input)?.any();
        self.handle(any)
    }

    fn channels(&mut self, input: Resource<StreamInput>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&input)?.channels())
    }

    fn latency(&mut self, input: Resource<StreamInput>) -> wasmtime::Result<f64> {
        Ok(self.table.get(&input)?.latency())
    }

    fn drop(&mut self, input: Resource<StreamInput>) -> wasmtime::Result<()> {
        self.table.delete(input)?;
        Ok(())
    }
}

impl<T: AudioCtxView + 'static> HostStreamInputNodeWithStore<T> for HasAudio {
    /// The samples are made into blocks in the call; only queueing them waits.
    async fn write(
        accessor: &Accessor<T, Self>,
        input: Resource<StreamInput>,
        samples: Vec<f32>,
    ) -> wasmtime::Result<Result<(), AudioError>> {
        let (writer, blocks) = accessor.with(|mut access| {
            let input = access.get().table.get(&input)?;
            Ok::<_, wasmtime::Error>((input.writer(), input.blocks_of(&samples)?))
        })?;
        Ok(writer.write(blocks).await.map_err(|WriteError::Closed| {
            AudioError::InvalidState("the context is closed".to_owned())
        }))
    }
}

impl HostMicrophoneNode for AudioCtx<'_> {
    fn node(&mut self, microphone: Resource<Microphone>) -> wasmtime::Result<Resource<AnyNode>> {
        let any = self.table.get(&microphone)?.any();
        self.handle(any)
    }

    fn drop(&mut self, microphone: Resource<Microphone>) -> wasmtime::Result<()> {
        self.table.delete(microphone)?;
        Ok(())
    }
}
