//! Each kind of node: the `audio-node` it hands out, its params, its attributes, and for a
//! scheduled source its start, its stop and its end.

use waos_capabilities::web_audio_api::node::{
    AnalyserNode, AudioBufferSourceNode, AudioNode, AudioScheduledSourceNode, BiquadFilterNode,
    ChannelMergerNode, ChannelSplitterNode, ConstantSourceNode, ConvolverNode, DelayNode,
    DynamicsCompressorNode, GainNode, IIRFilterNode, OscillatorNode, PannerNode, StereoPannerNode,
    WaveShaperNode,
};
use waos_capabilities::web_audio_api::{AudioBuffer, AudioParam, PeriodicWave};
use wasmtime::component::{Accessor, Resource};

use super::{AudioCtx, AudioCtxView, HasAudio};
use crate::audio::mistakes::trapping;
use crate::audio::{AnyNode, Node, Source};
use crate::bindings::wasdk::platform::audio::{
    BiquadFilterType, DistanceModelType, FrequencyResponse, HostAnalyserNode,
    HostAudioBufferSourceNode, HostAudioBufferSourceNodeWithStore, HostBiquadFilterNode,
    HostChannelMergerNode, HostChannelSplitterNode, HostConstantSourceNode,
    HostConstantSourceNodeWithStore, HostConvolverNode, HostDelayNode, HostDynamicsCompressorNode,
    HostGainNode, HostIirFilterNode, HostOscillatorNode, HostOscillatorNodeWithStore,
    HostPannerNode, HostStereoPannerNode, HostWaveShaperNode, OscillatorType, OverSampleType,
    PanningModelType,
};

impl AudioCtx<'_> {
    /// The node behind `node`, of kind `N`.
    fn kind<N: AudioNode + Send + 'static>(
        &self,
        node: &Resource<Node<N>>,
    ) -> wasmtime::Result<&Node<N>> {
        Ok(self.table.get(node)?)
    }

    /// `node`'s kind's node as an `audio-node`.
    fn any_of<N: AudioNode + Send + 'static>(
        &mut self,
        node: &Resource<Node<N>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        let any = self.kind(node)?.any();
        self.handle(any)
    }

    /// The param `param` reads off the node behind `node`, as a new handle on it.
    fn param_of<N: AudioNode + Send + 'static>(
        &mut self,
        node: &Resource<Node<N>>,
        param: impl FnOnce(&N) -> &AudioParam,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.kind(node)?.with(|node| param(node).clone());
        self.handle(param)
    }

    /// `call` with the node behind `node`, which may panic at the program's mistake.
    fn on<N: AudioNode + Send + 'static, T>(
        &self,
        node: &Resource<Node<N>>,
        call: impl FnOnce(&mut N) -> T,
    ) -> wasmtime::Result<T> {
        let node = self.kind(node)?;
        trapping(|| node.with(call))
    }

    /// The scheduled source behind `source`, of kind `N`.
    fn source<N: Send + 'static>(
        &self,
        source: &Resource<Source<N>>,
    ) -> wasmtime::Result<&Source<N>> {
        Ok(self.table.get(source)?)
    }

    /// The param `param` reads off the source behind `source`, as a new handle on it.
    fn source_param<N: AudioScheduledSourceNode + Send + 'static>(
        &mut self,
        source: &Resource<Source<N>>,
        param: impl FnOnce(&N) -> &AudioParam,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.source(source)?.node().with(|node| param(node).clone());
        self.handle(param)
    }

    /// The source behind `source` as an `audio-node`.
    fn source_any<N: AudioScheduledSourceNode + Send + 'static>(
        &mut self,
        source: &Resource<Source<N>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        let any = self.source(source)?.any();
        self.handle(any)
    }

    /// `call` with the source behind `source`, which may panic at the program's mistake.
    fn on_source<N: AudioScheduledSourceNode + Send + 'static, T>(
        &self,
        source: &Resource<Source<N>>,
        call: impl FnOnce(&mut N) -> T,
    ) -> wasmtime::Result<T> {
        let source = self.source(source)?;
        trapping(|| source.node().with(call))
    }

    /// A copy of the buffer behind `buffer`, which a node takes.
    fn buffer_copy(&self, buffer: &Resource<AudioBuffer>) -> wasmtime::Result<AudioBuffer> {
        Ok(self.table.get(buffer)?.clone())
    }

    /// A buffer a node holds, as a new handle on a copy of it.
    fn buffer_handle(
        &mut self,
        buffer: Option<AudioBuffer>,
    ) -> wasmtime::Result<Option<Resource<AudioBuffer>>> {
        buffer.map(|buffer| self.handle(buffer)).transpose()
    }
}

/// The frequency response `respond` writes for `frequency_hz`.
fn frequency_response(
    frequency_hz: &[f32],
    respond: impl FnOnce(&[f32], &mut [f32], &mut [f32]),
) -> FrequencyResponse {
    let mut magnitude = vec![0.0; frequency_hz.len()];
    let mut phase = vec![0.0; frequency_hz.len()];
    respond(frequency_hz, &mut magnitude, &mut phase);
    FrequencyResponse { magnitude, phase }
}

/// Waits for the end of the source behind `source`.
async fn ended<T: AudioCtxView + 'static, N: AudioScheduledSourceNode + Send + 'static>(
    accessor: &Accessor<T, HasAudio>,
    source: Resource<Source<N>>,
) -> wasmtime::Result<()> {
    let ended = accessor
        .with(|mut access| Ok::<_, wasmtime::Error>(access.get().source(&source)?.ended()))?;
    ended.await;
    Ok(())
}

impl HostGainNode for AudioCtx<'_> {
    fn node(&mut self, gain: Resource<Node<GainNode>>) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&gain)
    }

    fn gain(&mut self, gain: Resource<Node<GainNode>>) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&gain, GainNode::gain)
    }

    fn drop(&mut self, gain: Resource<Node<GainNode>>) -> wasmtime::Result<()> {
        self.table.delete(gain)?;
        Ok(())
    }
}

impl HostOscillatorNode for AudioCtx<'_> {
    fn node(
        &mut self,
        oscillator: Resource<Source<OscillatorNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.source_any(&oscillator)
    }

    fn type_(
        &mut self,
        oscillator: Resource<Source<OscillatorNode>>,
    ) -> wasmtime::Result<OscillatorType> {
        self.on_source(&oscillator, |oscillator| oscillator.type_().into())
    }

    fn set_type(
        &mut self,
        oscillator: Resource<Source<OscillatorNode>>,
        type_: OscillatorType,
    ) -> wasmtime::Result<()> {
        self.on_source(&oscillator, |oscillator| oscillator.set_type(type_.into()))
    }

    fn frequency(
        &mut self,
        oscillator: Resource<Source<OscillatorNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.source_param(&oscillator, OscillatorNode::frequency)
    }

    fn detune(
        &mut self,
        oscillator: Resource<Source<OscillatorNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.source_param(&oscillator, OscillatorNode::detune)
    }

    fn set_periodic_wave(
        &mut self,
        oscillator: Resource<Source<OscillatorNode>>,
        wave: Resource<PeriodicWave>,
    ) -> wasmtime::Result<()> {
        let wave = self.table.get(&wave)?.clone();
        self.on_source(&oscillator, |oscillator| oscillator.set_periodic_wave(wave))
    }

    fn start(
        &mut self,
        oscillator: Resource<Source<OscillatorNode>>,
        when: f64,
    ) -> wasmtime::Result<()> {
        self.source(&oscillator)?
            .start(when, |oscillator| oscillator.start_at(when))
    }

    fn stop(
        &mut self,
        oscillator: Resource<Source<OscillatorNode>>,
        when: f64,
    ) -> wasmtime::Result<()> {
        self.on_source(&oscillator, |oscillator| oscillator.stop_at(when))
    }

    fn drop(&mut self, oscillator: Resource<Source<OscillatorNode>>) -> wasmtime::Result<()> {
        self.table.delete(oscillator)?;
        Ok(())
    }
}

impl<T: AudioCtxView + 'static> HostOscillatorNodeWithStore<T> for HasAudio {
    async fn ended(
        accessor: &Accessor<T, Self>,
        oscillator: Resource<Source<OscillatorNode>>,
    ) -> wasmtime::Result<()> {
        ended(accessor, oscillator).await
    }
}

impl HostAudioBufferSourceNode for AudioCtx<'_> {
    fn node(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.source_any(&source)
    }

    fn buffer(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
    ) -> wasmtime::Result<Option<Resource<AudioBuffer>>> {
        let buffer = self.on_source(&source, |source| source.buffer().cloned())?;
        self.buffer_handle(buffer)
    }

    fn set_buffer(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
        buffer: Resource<AudioBuffer>,
    ) -> wasmtime::Result<()> {
        let buffer = self.buffer_copy(&buffer)?;
        self.on_source(&source, |source| source.set_buffer(buffer))
    }

    fn playback_rate(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.source_param(&source, AudioBufferSourceNode::playback_rate)
    }

    fn detune(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.source_param(&source, AudioBufferSourceNode::detune)
    }

    fn loop_(&mut self, source: Resource<Source<AudioBufferSourceNode>>) -> wasmtime::Result<bool> {
        self.on_source(&source, |source| source.loop_())
    }

    fn set_loop(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
        value: bool,
    ) -> wasmtime::Result<()> {
        self.on_source(&source, |source| source.set_loop(value))
    }

    fn loop_start(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
    ) -> wasmtime::Result<f64> {
        self.on_source(&source, |source| source.loop_start())
    }

    fn set_loop_start(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on_source(&source, |source| source.set_loop_start(value))
    }

    fn loop_end(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
    ) -> wasmtime::Result<f64> {
        self.on_source(&source, |source| source.loop_end())
    }

    fn set_loop_end(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on_source(&source, |source| source.set_loop_end(value))
    }

    fn start(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
        when: f64,
        offset: f64,
        duration: Option<f64>,
    ) -> wasmtime::Result<()> {
        self.source(&source)?.start(when, |source| match duration {
            Some(duration) => source.start_at_with_offset_and_duration(when, offset, duration),
            None => source.start_at_with_offset(when, offset),
        })
    }

    fn stop(
        &mut self,
        source: Resource<Source<AudioBufferSourceNode>>,
        when: f64,
    ) -> wasmtime::Result<()> {
        self.on_source(&source, |source| source.stop_at(when))
    }

    fn drop(&mut self, source: Resource<Source<AudioBufferSourceNode>>) -> wasmtime::Result<()> {
        self.table.delete(source)?;
        Ok(())
    }
}

impl<T: AudioCtxView + 'static> HostAudioBufferSourceNodeWithStore<T> for HasAudio {
    async fn ended(
        accessor: &Accessor<T, Self>,
        source: Resource<Source<AudioBufferSourceNode>>,
    ) -> wasmtime::Result<()> {
        ended(accessor, source).await
    }
}

impl HostConstantSourceNode for AudioCtx<'_> {
    fn node(
        &mut self,
        source: Resource<Source<ConstantSourceNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.source_any(&source)
    }

    fn offset(
        &mut self,
        source: Resource<Source<ConstantSourceNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.source_param(&source, ConstantSourceNode::offset)
    }

    fn start(
        &mut self,
        source: Resource<Source<ConstantSourceNode>>,
        when: f64,
    ) -> wasmtime::Result<()> {
        self.source(&source)?
            .start(when, |source| source.start_at(when))
    }

    fn stop(
        &mut self,
        source: Resource<Source<ConstantSourceNode>>,
        when: f64,
    ) -> wasmtime::Result<()> {
        self.on_source(&source, |source| source.stop_at(when))
    }

    fn drop(&mut self, source: Resource<Source<ConstantSourceNode>>) -> wasmtime::Result<()> {
        self.table.delete(source)?;
        Ok(())
    }
}

impl<T: AudioCtxView + 'static> HostConstantSourceNodeWithStore<T> for HasAudio {
    async fn ended(
        accessor: &Accessor<T, Self>,
        source: Resource<Source<ConstantSourceNode>>,
    ) -> wasmtime::Result<()> {
        ended(accessor, source).await
    }
}

impl HostDelayNode for AudioCtx<'_> {
    fn node(&mut self, delay: Resource<Node<DelayNode>>) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&delay)
    }

    fn delay_time(
        &mut self,
        delay: Resource<Node<DelayNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&delay, DelayNode::delay_time)
    }

    fn drop(&mut self, delay: Resource<Node<DelayNode>>) -> wasmtime::Result<()> {
        self.table.delete(delay)?;
        Ok(())
    }
}

impl HostBiquadFilterNode for AudioCtx<'_> {
    fn node(
        &mut self,
        filter: Resource<Node<BiquadFilterNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&filter)
    }

    fn type_(
        &mut self,
        filter: Resource<Node<BiquadFilterNode>>,
    ) -> wasmtime::Result<BiquadFilterType> {
        self.on(&filter, |filter| filter.type_().into())
    }

    fn set_type(
        &mut self,
        filter: Resource<Node<BiquadFilterNode>>,
        type_: BiquadFilterType,
    ) -> wasmtime::Result<()> {
        self.on(&filter, |filter| filter.set_type(type_.into()))
    }

    fn frequency(
        &mut self,
        filter: Resource<Node<BiquadFilterNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&filter, BiquadFilterNode::frequency)
    }

    fn detune(
        &mut self,
        filter: Resource<Node<BiquadFilterNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&filter, BiquadFilterNode::detune)
    }

    fn q(
        &mut self,
        filter: Resource<Node<BiquadFilterNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&filter, BiquadFilterNode::q)
    }

    fn gain(
        &mut self,
        filter: Resource<Node<BiquadFilterNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&filter, BiquadFilterNode::gain)
    }

    fn get_frequency_response(
        &mut self,
        filter: Resource<Node<BiquadFilterNode>>,
        frequency_hz: Vec<f32>,
    ) -> wasmtime::Result<FrequencyResponse> {
        self.on(&filter, |filter| {
            frequency_response(&frequency_hz, |hz, magnitude, phase| {
                filter.get_frequency_response(hz, magnitude, phase);
            })
        })
    }

    fn drop(&mut self, filter: Resource<Node<BiquadFilterNode>>) -> wasmtime::Result<()> {
        self.table.delete(filter)?;
        Ok(())
    }
}

impl HostIirFilterNode for AudioCtx<'_> {
    fn node(
        &mut self,
        filter: Resource<Node<IIRFilterNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&filter)
    }

    fn get_frequency_response(
        &mut self,
        filter: Resource<Node<IIRFilterNode>>,
        frequency_hz: Vec<f32>,
    ) -> wasmtime::Result<FrequencyResponse> {
        self.on(&filter, |filter| {
            frequency_response(&frequency_hz, |hz, magnitude, phase| {
                filter.get_frequency_response(hz, magnitude, phase);
            })
        })
    }

    fn drop(&mut self, filter: Resource<Node<IIRFilterNode>>) -> wasmtime::Result<()> {
        self.table.delete(filter)?;
        Ok(())
    }
}

impl HostDynamicsCompressorNode for AudioCtx<'_> {
    fn node(
        &mut self,
        compressor: Resource<Node<DynamicsCompressorNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&compressor)
    }

    fn threshold(
        &mut self,
        compressor: Resource<Node<DynamicsCompressorNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&compressor, DynamicsCompressorNode::threshold)
    }

    fn knee(
        &mut self,
        compressor: Resource<Node<DynamicsCompressorNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&compressor, DynamicsCompressorNode::knee)
    }

    fn ratio(
        &mut self,
        compressor: Resource<Node<DynamicsCompressorNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&compressor, DynamicsCompressorNode::ratio)
    }

    fn attack(
        &mut self,
        compressor: Resource<Node<DynamicsCompressorNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&compressor, DynamicsCompressorNode::attack)
    }

    fn release(
        &mut self,
        compressor: Resource<Node<DynamicsCompressorNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&compressor, DynamicsCompressorNode::release)
    }

    fn reduction(
        &mut self,
        compressor: Resource<Node<DynamicsCompressorNode>>,
    ) -> wasmtime::Result<f32> {
        self.on(&compressor, |compressor| compressor.reduction())
    }

    fn drop(&mut self, compressor: Resource<Node<DynamicsCompressorNode>>) -> wasmtime::Result<()> {
        self.table.delete(compressor)?;
        Ok(())
    }
}

impl HostConvolverNode for AudioCtx<'_> {
    fn node(
        &mut self,
        convolver: Resource<Node<ConvolverNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&convolver)
    }

    fn buffer(
        &mut self,
        convolver: Resource<Node<ConvolverNode>>,
    ) -> wasmtime::Result<Option<Resource<AudioBuffer>>> {
        let buffer = self.on(&convolver, |convolver| convolver.buffer().cloned())?;
        self.buffer_handle(buffer)
    }

    fn set_buffer(
        &mut self,
        convolver: Resource<Node<ConvolverNode>>,
        buffer: Resource<AudioBuffer>,
    ) -> wasmtime::Result<()> {
        let buffer = self.buffer_copy(&buffer)?;
        self.on(&convolver, |convolver| convolver.set_buffer(buffer))
    }

    fn normalize(&mut self, convolver: Resource<Node<ConvolverNode>>) -> wasmtime::Result<bool> {
        self.on(&convolver, |convolver| convolver.normalize())
    }

    fn set_normalize(
        &mut self,
        convolver: Resource<Node<ConvolverNode>>,
        value: bool,
    ) -> wasmtime::Result<()> {
        self.on(&convolver, |convolver| convolver.set_normalize(value))
    }

    fn drop(&mut self, convolver: Resource<Node<ConvolverNode>>) -> wasmtime::Result<()> {
        self.table.delete(convolver)?;
        Ok(())
    }
}

impl HostWaveShaperNode for AudioCtx<'_> {
    fn node(
        &mut self,
        shaper: Resource<Node<WaveShaperNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&shaper)
    }

    fn curve(
        &mut self,
        shaper: Resource<Node<WaveShaperNode>>,
    ) -> wasmtime::Result<Option<Vec<f32>>> {
        self.on(&shaper, |shaper| shaper.curve().map(<[f32]>::to_vec))
    }

    fn set_curve(
        &mut self,
        shaper: Resource<Node<WaveShaperNode>>,
        curve: Vec<f32>,
    ) -> wasmtime::Result<()> {
        self.on(&shaper, |shaper| shaper.set_curve(curve))
    }

    fn oversample(
        &mut self,
        shaper: Resource<Node<WaveShaperNode>>,
    ) -> wasmtime::Result<OverSampleType> {
        self.on(&shaper, |shaper| shaper.oversample().into())
    }

    fn set_oversample(
        &mut self,
        shaper: Resource<Node<WaveShaperNode>>,
        oversample: OverSampleType,
    ) -> wasmtime::Result<()> {
        self.on(&shaper, |shaper| shaper.set_oversample(oversample.into()))
    }

    fn drop(&mut self, shaper: Resource<Node<WaveShaperNode>>) -> wasmtime::Result<()> {
        self.table.delete(shaper)?;
        Ok(())
    }
}

impl HostStereoPannerNode for AudioCtx<'_> {
    fn node(
        &mut self,
        panner: Resource<Node<StereoPannerNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&panner)
    }

    fn pan(
        &mut self,
        panner: Resource<Node<StereoPannerNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&panner, StereoPannerNode::pan)
    }

    fn drop(&mut self, panner: Resource<Node<StereoPannerNode>>) -> wasmtime::Result<()> {
        self.table.delete(panner)?;
        Ok(())
    }
}

impl HostPannerNode for AudioCtx<'_> {
    fn node(&mut self, panner: Resource<Node<PannerNode>>) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&panner)
    }

    fn panning_model(
        &mut self,
        panner: Resource<Node<PannerNode>>,
    ) -> wasmtime::Result<PanningModelType> {
        self.on(&panner, |panner| panner.panning_model().into())
    }

    fn set_panning_model(
        &mut self,
        panner: Resource<Node<PannerNode>>,
        model: PanningModelType,
    ) -> wasmtime::Result<()> {
        self.on(&panner, |panner| panner.set_panning_model(model.into()))
    }

    fn distance_model(
        &mut self,
        panner: Resource<Node<PannerNode>>,
    ) -> wasmtime::Result<DistanceModelType> {
        self.on(&panner, |panner| panner.distance_model().into())
    }

    fn set_distance_model(
        &mut self,
        panner: Resource<Node<PannerNode>>,
        model: DistanceModelType,
    ) -> wasmtime::Result<()> {
        self.on(&panner, |panner| panner.set_distance_model(model.into()))
    }

    fn position_x(
        &mut self,
        panner: Resource<Node<PannerNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&panner, PannerNode::position_x)
    }

    fn position_y(
        &mut self,
        panner: Resource<Node<PannerNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&panner, PannerNode::position_y)
    }

    fn position_z(
        &mut self,
        panner: Resource<Node<PannerNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&panner, PannerNode::position_z)
    }

    fn orientation_x(
        &mut self,
        panner: Resource<Node<PannerNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&panner, PannerNode::orientation_x)
    }

    fn orientation_y(
        &mut self,
        panner: Resource<Node<PannerNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&panner, PannerNode::orientation_y)
    }

    fn orientation_z(
        &mut self,
        panner: Resource<Node<PannerNode>>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        self.param_of(&panner, PannerNode::orientation_z)
    }

    fn ref_distance(&mut self, panner: Resource<Node<PannerNode>>) -> wasmtime::Result<f64> {
        self.on(&panner, |panner| panner.ref_distance())
    }

    fn set_ref_distance(
        &mut self,
        panner: Resource<Node<PannerNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on(&panner, |panner| panner.set_ref_distance(value))
    }

    fn max_distance(&mut self, panner: Resource<Node<PannerNode>>) -> wasmtime::Result<f64> {
        self.on(&panner, |panner| panner.max_distance())
    }

    fn set_max_distance(
        &mut self,
        panner: Resource<Node<PannerNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on(&panner, |panner| panner.set_max_distance(value))
    }

    fn rolloff_factor(&mut self, panner: Resource<Node<PannerNode>>) -> wasmtime::Result<f64> {
        self.on(&panner, |panner| panner.rolloff_factor())
    }

    fn set_rolloff_factor(
        &mut self,
        panner: Resource<Node<PannerNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on(&panner, |panner| panner.set_rolloff_factor(value))
    }

    fn cone_inner_angle(&mut self, panner: Resource<Node<PannerNode>>) -> wasmtime::Result<f64> {
        self.on(&panner, |panner| panner.cone_inner_angle())
    }

    fn set_cone_inner_angle(
        &mut self,
        panner: Resource<Node<PannerNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on(&panner, |panner| panner.set_cone_inner_angle(value))
    }

    fn cone_outer_angle(&mut self, panner: Resource<Node<PannerNode>>) -> wasmtime::Result<f64> {
        self.on(&panner, |panner| panner.cone_outer_angle())
    }

    fn set_cone_outer_angle(
        &mut self,
        panner: Resource<Node<PannerNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on(&panner, |panner| panner.set_cone_outer_angle(value))
    }

    fn cone_outer_gain(&mut self, panner: Resource<Node<PannerNode>>) -> wasmtime::Result<f64> {
        self.on(&panner, |panner| panner.cone_outer_gain())
    }

    fn set_cone_outer_gain(
        &mut self,
        panner: Resource<Node<PannerNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on(&panner, |panner| panner.set_cone_outer_gain(value))
    }

    fn drop(&mut self, panner: Resource<Node<PannerNode>>) -> wasmtime::Result<()> {
        self.table.delete(panner)?;
        Ok(())
    }
}

impl HostAnalyserNode for AudioCtx<'_> {
    fn node(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&analyser)
    }

    fn fft_size(&mut self, analyser: Resource<Node<AnalyserNode>>) -> wasmtime::Result<u32> {
        self.on(&analyser, |analyser| analyser.fft_size() as u32)
    }

    fn set_fft_size(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
        size: u32,
    ) -> wasmtime::Result<()> {
        self.on(&analyser, |analyser| analyser.set_fft_size(size as usize))
    }

    fn frequency_bin_count(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
    ) -> wasmtime::Result<u32> {
        self.on(&analyser, |analyser| analyser.frequency_bin_count() as u32)
    }

    fn min_decibels(&mut self, analyser: Resource<Node<AnalyserNode>>) -> wasmtime::Result<f64> {
        self.on(&analyser, |analyser| analyser.min_decibels())
    }

    fn set_min_decibels(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on(&analyser, |analyser| analyser.set_min_decibels(value))
    }

    fn max_decibels(&mut self, analyser: Resource<Node<AnalyserNode>>) -> wasmtime::Result<f64> {
        self.on(&analyser, |analyser| analyser.max_decibels())
    }

    fn set_max_decibels(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on(&analyser, |analyser| analyser.set_max_decibels(value))
    }

    fn smoothing_time_constant(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
    ) -> wasmtime::Result<f64> {
        self.on(&analyser, |analyser| analyser.smoothing_time_constant())
    }

    fn set_smoothing_time_constant(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
        value: f64,
    ) -> wasmtime::Result<()> {
        self.on(&analyser, |analyser| {
            analyser.set_smoothing_time_constant(value)
        })
    }

    fn get_float_frequency_data(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
    ) -> wasmtime::Result<Vec<f32>> {
        self.on(&analyser, |analyser| {
            let mut data = vec![0.0; analyser.frequency_bin_count()];
            analyser.get_float_frequency_data(&mut data);
            data
        })
    }

    fn get_byte_frequency_data(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
    ) -> wasmtime::Result<Vec<u8>> {
        self.on(&analyser, |analyser| {
            let mut data = vec![0; analyser.frequency_bin_count()];
            analyser.get_byte_frequency_data(&mut data);
            data
        })
    }

    fn get_float_time_domain_data(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
    ) -> wasmtime::Result<Vec<f32>> {
        self.on(&analyser, |analyser| {
            let mut data = vec![0.0; analyser.fft_size()];
            analyser.get_float_time_domain_data(&mut data);
            data
        })
    }

    fn get_byte_time_domain_data(
        &mut self,
        analyser: Resource<Node<AnalyserNode>>,
    ) -> wasmtime::Result<Vec<u8>> {
        self.on(&analyser, |analyser| {
            let mut data = vec![0; analyser.fft_size()];
            analyser.get_byte_time_domain_data(&mut data);
            data
        })
    }

    fn drop(&mut self, analyser: Resource<Node<AnalyserNode>>) -> wasmtime::Result<()> {
        self.table.delete(analyser)?;
        Ok(())
    }
}

impl HostChannelSplitterNode for AudioCtx<'_> {
    fn node(
        &mut self,
        splitter: Resource<Node<ChannelSplitterNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&splitter)
    }

    fn drop(&mut self, splitter: Resource<Node<ChannelSplitterNode>>) -> wasmtime::Result<()> {
        self.table.delete(splitter)?;
        Ok(())
    }
}

impl HostChannelMergerNode for AudioCtx<'_> {
    fn node(
        &mut self,
        merger: Resource<Node<ChannelMergerNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        self.any_of(&merger)
    }

    fn drop(&mut self, merger: Resource<Node<ChannelMergerNode>>) -> wasmtime::Result<()> {
        self.table.delete(merger)?;
        Ok(())
    }
}
