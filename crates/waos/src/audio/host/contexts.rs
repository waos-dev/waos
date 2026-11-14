//! Each kind of context, and what every context is: `base-audio-context`, the graph's clock, its
//! state and the factories of its nodes and values; `audio-context`, which hands out its base,
//! with its output, its suspending, resuming and closing, and the platform's own inputs; and
//! `offline-audio-context`, which hands out its base, with its length and its rendering.

use waos_capabilities::web_audio_api::context::{BaseAudioContext, ConcreteBaseAudioContext};
use waos_capabilities::web_audio_api::node;
use waos_capabilities::web_audio_api::{
    AudioBuffer, AudioListener, PeriodicWave, PeriodicWaveOptions,
};
use wasmtime::component::{Access, Accessor, Resource, StreamReader};

use super::{AudioCtx, AudioCtxView, HasAudio};
use crate::audio::mistakes::trapping;
use crate::audio::{
    BaseContext, Context, MOST_SAMPLES, Microphone, Node, OfflineContext, ProgramAudio, Source,
    StreamInput,
};
use crate::bindings::wasdk::platform::audio::{
    AudioContextState, AudioError, HostAudioContext, HostAudioContextWithStore,
    HostBaseAudioContext, HostBaseAudioContextWithStore, HostOfflineAudioContext,
    HostOfflineAudioContextWithStore,
};
use crate::stream::stream_of;

impl AudioCtx<'_> {
    /// The context behind `context`.
    fn context(&self, context: &Resource<Context>) -> wasmtime::Result<&Context> {
        Ok(self.table.get(context)?)
    }

    /// The context behind `base`, as any context.
    fn base_context(&self, base: &Resource<BaseContext>) -> wasmtime::Result<&BaseContext> {
        Ok(self.table.get(base)?)
    }

    /// A node of kind `N` made on `base` by `make`, as its kind's handle.
    fn made<N: node::AudioNode + Send + 'static>(
        &mut self,
        base: &Resource<BaseContext>,
        make: impl FnOnce(&ConcreteBaseAudioContext) -> N,
    ) -> wasmtime::Result<Resource<Node<N>>> {
        let context = self.base_context(base)?.context();
        let node = trapping(|| make(context))?;
        self.handle(Node::new(node))
    }

    /// A scheduled source of kind `N` made on `base` by `make`, as its kind's handle.
    fn made_source<N: node::AudioScheduledSourceNode + Send + 'static>(
        &mut self,
        base: &Resource<BaseContext>,
        make: impl FnOnce(&ConcreteBaseAudioContext) -> N,
    ) -> wasmtime::Result<Resource<Source<N>>> {
        let base = self.base_context(base)?;
        let node = trapping(|| make(base.context()))?;
        let source = Source::new(node, base.sound());
        self.handle(source)
    }
}

impl HostBaseAudioContext for AudioCtx<'_> {
    fn destination(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Node<node::AudioDestinationNode>>> {
        self.made(&base, |context| context.destination())
    }

    fn sample_rate(&mut self, base: Resource<BaseContext>) -> wasmtime::Result<f32> {
        Ok(self.base_context(&base)?.context().sample_rate())
    }

    fn current_time(&mut self, base: Resource<BaseContext>) -> wasmtime::Result<f64> {
        Ok(self.base_context(&base)?.context().current_time())
    }

    fn listener(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<AudioListener>> {
        let listener = self.base_context(&base)?.context().listener();
        self.handle(listener)
    }

    fn state(&mut self, base: Resource<BaseContext>) -> wasmtime::Result<AudioContextState> {
        Ok(self.base_context(&base)?.context().state().into())
    }

    fn create_analyser(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Node<node::AnalyserNode>>> {
        self.made(&base, |context| context.create_analyser())
    }

    fn create_biquad_filter(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Node<node::BiquadFilterNode>>> {
        self.made(&base, |context| context.create_biquad_filter())
    }

    /// A buffer the program could hold itself: no more samples than its memory could, so a
    /// buffer asked for beyond what wasm reaches traps rather than exhausting the machine.
    fn create_buffer(
        &mut self,
        base: Resource<BaseContext>,
        number_of_channels: u32,
        length: u32,
        sample_rate: f32,
    ) -> wasmtime::Result<Resource<AudioBuffer>> {
        let context = self.base_context(&base)?.context();
        let samples = u64::from(number_of_channels) * u64::from(length);
        wasmtime::ensure!(
            samples <= MOST_SAMPLES,
            "NotSupportedError - a buffer of {number_of_channels} channels of {length} samples"
        );
        let buffer = trapping(|| {
            context.create_buffer(number_of_channels as usize, length as usize, sample_rate)
        })?;
        self.handle(buffer)
    }

    fn create_buffer_source(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Source<node::AudioBufferSourceNode>>> {
        self.made_source(&base, |context| context.create_buffer_source())
    }

    fn create_channel_merger(
        &mut self,
        base: Resource<BaseContext>,
        number_of_inputs: u32,
    ) -> wasmtime::Result<Resource<Node<node::ChannelMergerNode>>> {
        self.made(&base, |context| {
            context.create_channel_merger(number_of_inputs as usize)
        })
    }

    fn create_channel_splitter(
        &mut self,
        base: Resource<BaseContext>,
        number_of_outputs: u32,
    ) -> wasmtime::Result<Resource<Node<node::ChannelSplitterNode>>> {
        self.made(&base, |context| {
            context.create_channel_splitter(number_of_outputs as usize)
        })
    }

    fn create_constant_source(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Source<node::ConstantSourceNode>>> {
        self.made_source(&base, |context| context.create_constant_source())
    }

    fn create_convolver(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Node<node::ConvolverNode>>> {
        self.made(&base, |context| context.create_convolver())
    }

    fn create_delay(
        &mut self,
        base: Resource<BaseContext>,
        max_delay_time: f64,
    ) -> wasmtime::Result<Resource<Node<node::DelayNode>>> {
        self.made(&base, |context| context.create_delay(max_delay_time))
    }

    fn create_dynamics_compressor(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Node<node::DynamicsCompressorNode>>> {
        self.made(&base, |context| context.create_dynamics_compressor())
    }

    fn create_gain(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Node<node::GainNode>>> {
        self.made(&base, |context| context.create_gain())
    }

    fn create_iir_filter(
        &mut self,
        base: Resource<BaseContext>,
        feedforward: Vec<f64>,
        feedback: Vec<f64>,
    ) -> wasmtime::Result<Resource<Node<node::IIRFilterNode>>> {
        self.made(&base, |context| {
            context.create_iir_filter(feedforward, feedback)
        })
    }

    fn create_oscillator(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Source<node::OscillatorNode>>> {
        self.made_source(&base, |context| context.create_oscillator())
    }

    fn create_panner(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Node<node::PannerNode>>> {
        self.made(&base, |context| context.create_panner())
    }

    fn create_periodic_wave(
        &mut self,
        base: Resource<BaseContext>,
        real: Vec<f32>,
        imag: Vec<f32>,
        disable_normalization: bool,
    ) -> wasmtime::Result<Resource<PeriodicWave>> {
        let context = self.base_context(&base)?.context();
        let options = PeriodicWaveOptions {
            real: Some(real),
            imag: Some(imag),
            disable_normalization,
        };
        let wave = trapping(|| context.create_periodic_wave(options))?;
        self.handle(wave)
    }

    fn create_stereo_panner(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Node<node::StereoPannerNode>>> {
        self.made(&base, |context| context.create_stereo_panner())
    }

    fn create_wave_shaper(
        &mut self,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<Resource<Node<node::WaveShaperNode>>> {
        self.made(&base, |context| context.create_wave_shaper())
    }

    fn drop(&mut self, base: Resource<BaseContext>) -> wasmtime::Result<()> {
        self.table.delete(base)?;
        Ok(())
    }
}

impl<T: AudioCtxView + 'static> HostBaseAudioContextWithStore<T> for HasAudio {
    /// Each state enters the program's stream as it reads it.
    fn state_changes(
        mut access: Access<T, Self>,
        base: Resource<BaseContext>,
    ) -> wasmtime::Result<StreamReader<AudioContextState>> {
        let changes = access.get().base_context(&base)?.state_changes();
        stream_of(access, changes, |_: &mut T, state| Ok(state.into()))
    }

    async fn decode_audio_data(
        accessor: &Accessor<T, Self>,
        base: Resource<BaseContext>,
        data: Vec<u8>,
    ) -> wasmtime::Result<Result<Resource<AudioBuffer>, AudioError>> {
        let base = accessor.with(|mut access| {
            Ok::<_, wasmtime::Error>(access.get().base_context(&base)?.clone())
        })?;
        match base.decode(data).await? {
            Ok(buffer) => accessor.with(|mut access| Ok(Ok(access.get().handle(buffer)?))),
            Err(error) => Ok(Err(error)),
        }
    }
}

impl HostAudioContext for AudioCtx<'_> {
    fn base(&mut self, context: Resource<Context>) -> wasmtime::Result<Resource<BaseContext>> {
        let base = BaseContext::Playing(self.context(&context)?.clone());
        self.handle(base)
    }

    fn base_latency(&mut self, context: Resource<Context>) -> wasmtime::Result<f64> {
        Ok(self.context(&context)?.context().base_latency())
    }

    fn output_latency(&mut self, context: Resource<Context>) -> wasmtime::Result<f64> {
        let context = self.context(&context)?;
        trapping(|| context.context().output_latency())
    }

    fn sink_id(&mut self, context: Resource<Context>) -> wasmtime::Result<String> {
        Ok(self.context(&context)?.sink_id())
    }

    fn create_stream_input(
        &mut self,
        context: Resource<Context>,
        channels: u32,
    ) -> wasmtime::Result<Resource<StreamInput>> {
        let context = self.context(&context)?;
        let playing = context.sound().play();
        let input = StreamInput::new(context.context(), channels, playing)?;
        self.handle(input)
    }

    fn drop(&mut self, context: Resource<Context>) -> wasmtime::Result<()> {
        self.table.delete(context)?;
        Ok(())
    }
}

impl<T: AudioCtxView + 'static> HostAudioContextWithStore<T> for HasAudio {
    async fn set_sink_id(
        accessor: &Accessor<T, Self>,
        context: Resource<Context>,
        sink_id: String,
    ) -> wasmtime::Result<Result<(), AudioError>> {
        let (audio, context) = taken(accessor, &context)?;
        audio.set_sink_id(context, sink_id).await
    }

    async fn suspend(
        accessor: &Accessor<T, Self>,
        context: Resource<Context>,
    ) -> wasmtime::Result<Result<(), AudioError>> {
        let (_, context) = taken(accessor, &context)?;
        Ok(context.suspend().await)
    }

    async fn resume(
        accessor: &Accessor<T, Self>,
        context: Resource<Context>,
    ) -> wasmtime::Result<Result<(), AudioError>> {
        let (_, context) = taken(accessor, &context)?;
        Ok(context.resume().await)
    }

    async fn close(
        accessor: &Accessor<T, Self>,
        context: Resource<Context>,
    ) -> wasmtime::Result<()> {
        let (_, context) = taken(accessor, &context)?;
        context.close().await;
        Ok(())
    }

    /// Asks the user at the first use, through the program's microphone permission.
    async fn open_microphone(
        accessor: &Accessor<T, Self>,
        context: Resource<Context>,
    ) -> wasmtime::Result<Result<Resource<Microphone>, AudioError>> {
        let (audio, context, asked) = accessor.with(|mut access| {
            let ctx = access.get();
            let asked = ctx.audio.microphone_question(ctx.permissions)?;
            Ok::<_, wasmtime::Error>((ctx.audio.clone(), ctx.context(&context)?.clone(), asked))
        })?;
        let asked = match asked {
            Ok(asked) => asked,
            Err(error) => return Ok(Err(error)),
        };
        match audio.open_microphone(context, asked).await? {
            Ok(microphone) => {
                accessor.with(|mut access| Ok(Ok(access.get().handle(microphone)?)))
            }
            Err(error) => Ok(Err(error)),
        }
    }
}

impl HostOfflineAudioContext for AudioCtx<'_> {
    fn new(
        &mut self,
        number_of_channels: u32,
        length: u32,
        sample_rate: f32,
    ) -> wasmtime::Result<Resource<OfflineContext>> {
        let context = OfflineContext::new(number_of_channels, length, sample_rate)?;
        self.handle(context)
    }

    fn base(
        &mut self,
        context: Resource<OfflineContext>,
    ) -> wasmtime::Result<Resource<BaseContext>> {
        let base = BaseContext::Rendering(self.table.get(&context)?.clone());
        self.handle(base)
    }

    fn length(&mut self, context: Resource<OfflineContext>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&context)?.length())
    }

    fn drop(&mut self, context: Resource<OfflineContext>) -> wasmtime::Result<()> {
        self.table.delete(context)?;
        Ok(())
    }
}

impl<T: AudioCtxView + 'static> HostOfflineAudioContextWithStore<T> for HasAudio {
    async fn start_rendering(
        accessor: &Accessor<T, Self>,
        context: Resource<OfflineContext>,
    ) -> wasmtime::Result<Resource<AudioBuffer>> {
        let context = accessor.with(|mut access| {
            Ok::<_, wasmtime::Error>(access.get().table.get(&context)?.clone())
        })?;
        let rendered = context.render().await?;
        accessor.with(|mut access| access.get().handle(rendered))
    }
}

/// The program's sound and the context behind `context`, taken out of the store for a call that
/// outlasts it.
fn taken<T: AudioCtxView + 'static>(
    accessor: &Accessor<T, HasAudio>,
    context: &Resource<Context>,
) -> wasmtime::Result<(ProgramAudio, Context)> {
    accessor.with(|mut access| {
        let ctx = access.get();
        Ok((ctx.audio.clone(), ctx.context(context)?.clone()))
    })
}
