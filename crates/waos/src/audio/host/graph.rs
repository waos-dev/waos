//! What every node is, `audio-node`, with its connections and its channels; the context's
//! destination; and the graph's values: `audio-param` with its automation, `audio-buffer`,
//! `periodic-wave` and `audio-listener`.

use waos_capabilities::web_audio_api::node;
use waos_capabilities::web_audio_api::{AudioBuffer, AudioListener, AudioParam, PeriodicWave};
use wasmtime::component::Resource;

use super::AudioCtx;
use crate::audio::mistakes::trapping;
use crate::audio::{AnyNode, Node};
use crate::bindings::wasdk::platform::audio::{
    AutomationRate, ChannelCountMode, ChannelInterpretation, HostAudioBuffer,
    HostAudioDestinationNode, HostAudioListener, HostAudioNode, HostAudioParam, HostPeriodicWave,
};

impl AudioCtx<'_> {
    fn node(&self, node: &Resource<AnyNode>) -> wasmtime::Result<&AnyNode> {
        Ok(self.table.get(node)?)
    }

    fn param(&self, param: &Resource<AudioParam>) -> wasmtime::Result<&AudioParam> {
        Ok(self.table.get(param)?)
    }

    /// `call` with the node behind `source` and the node behind `destination`, which may be it.
    fn with_both(
        &self,
        source: &Resource<AnyNode>,
        destination: &Resource<AnyNode>,
        call: impl FnOnce(&dyn node::AudioNode, &dyn node::AudioNode),
    ) -> wasmtime::Result<()> {
        let (source, destination) = (self.node(source)?, self.node(destination)?);
        trapping(|| source.with_both(destination, call))
    }

    /// `call` with the node behind `source` and the param behind `destination`.
    fn with_param(
        &self,
        source: &Resource<AnyNode>,
        destination: &Resource<AudioParam>,
        call: impl FnOnce(&dyn node::AudioNode, &AudioParam),
    ) -> wasmtime::Result<()> {
        let (source, destination) = (self.node(source)?, self.param(destination)?);
        trapping(|| source.with(|source| call(source, destination)))
    }

    /// `call` with the node behind `node`, which may panic at the program's mistake.
    fn with_node<T>(
        &self,
        node: &Resource<AnyNode>,
        call: impl FnOnce(&dyn node::AudioNode) -> T,
    ) -> wasmtime::Result<T> {
        let node = self.node(node)?;
        trapping(|| node.with(call))
    }
}

impl HostAudioNode for AudioCtx<'_> {
    fn connect(
        &mut self,
        node: Resource<AnyNode>,
        destination: Resource<AnyNode>,
    ) -> wasmtime::Result<()> {
        self.with_both(&node, &destination, |node, destination| {
            node.connect(destination);
        })
    }

    fn connect_output_input(
        &mut self,
        node: Resource<AnyNode>,
        destination: Resource<AnyNode>,
        output: u32,
        input: u32,
    ) -> wasmtime::Result<()> {
        self.with_both(&node, &destination, |node, destination| {
            node.connect_from_output_to_input(destination, output as usize, input as usize);
        })
    }

    fn connect_param(
        &mut self,
        node: Resource<AnyNode>,
        destination: Resource<AudioParam>,
    ) -> wasmtime::Result<()> {
        self.with_param(&node, &destination, |node, param| {
            node.connect(param);
        })
    }

    fn connect_param_output(
        &mut self,
        node: Resource<AnyNode>,
        destination: Resource<AudioParam>,
        output: u32,
    ) -> wasmtime::Result<()> {
        self.with_param(&node, &destination, |node, param| {
            node.connect_from_output_to_input(param, output as usize, 0);
        })
    }

    fn disconnect(&mut self, node: Resource<AnyNode>) -> wasmtime::Result<()> {
        self.with_node(&node, |node| node.disconnect())
    }

    fn disconnect_output(&mut self, node: Resource<AnyNode>, output: u32) -> wasmtime::Result<()> {
        self.with_node(&node, |node| node.disconnect_output(output as usize))
    }

    fn disconnect_node(
        &mut self,
        node: Resource<AnyNode>,
        destination: Resource<AnyNode>,
    ) -> wasmtime::Result<()> {
        self.with_both(&node, &destination, |node, destination| {
            node.disconnect_dest(destination);
        })
    }

    fn disconnect_node_output(
        &mut self,
        node: Resource<AnyNode>,
        destination: Resource<AnyNode>,
        output: u32,
    ) -> wasmtime::Result<()> {
        self.with_both(&node, &destination, |node, destination| {
            node.disconnect_dest_from_output(destination, output as usize);
        })
    }

    fn disconnect_node_output_input(
        &mut self,
        node: Resource<AnyNode>,
        destination: Resource<AnyNode>,
        output: u32,
        input: u32,
    ) -> wasmtime::Result<()> {
        self.with_both(&node, &destination, |node, destination| {
            node.disconnect_dest_from_output_to_input(destination, output as usize, input as usize);
        })
    }

    fn disconnect_param(
        &mut self,
        node: Resource<AnyNode>,
        destination: Resource<AudioParam>,
    ) -> wasmtime::Result<()> {
        self.with_param(&node, &destination, |node, param| {
            node.disconnect_dest(param)
        })
    }

    fn disconnect_param_output(
        &mut self,
        node: Resource<AnyNode>,
        destination: Resource<AudioParam>,
        output: u32,
    ) -> wasmtime::Result<()> {
        self.with_param(&node, &destination, |node, param| {
            node.disconnect_dest_from_output(param, output as usize);
        })
    }

    fn number_of_inputs(&mut self, node: Resource<AnyNode>) -> wasmtime::Result<u32> {
        self.with_node(&node, |node| node.number_of_inputs() as u32)
    }

    fn number_of_outputs(&mut self, node: Resource<AnyNode>) -> wasmtime::Result<u32> {
        self.with_node(&node, |node| node.number_of_outputs() as u32)
    }

    fn channel_count(&mut self, node: Resource<AnyNode>) -> wasmtime::Result<u32> {
        self.with_node(&node, |node| node.channel_count() as u32)
    }

    fn set_channel_count(&mut self, node: Resource<AnyNode>, count: u32) -> wasmtime::Result<()> {
        self.with_node(&node, |node| node.set_channel_count(count as usize))
    }

    fn channel_count_mode(
        &mut self,
        node: Resource<AnyNode>,
    ) -> wasmtime::Result<ChannelCountMode> {
        self.with_node(&node, |node| node.channel_count_mode().into())
    }

    fn set_channel_count_mode(
        &mut self,
        node: Resource<AnyNode>,
        mode: ChannelCountMode,
    ) -> wasmtime::Result<()> {
        self.with_node(&node, |node| node.set_channel_count_mode(mode.into()))
    }

    fn channel_interpretation(
        &mut self,
        node: Resource<AnyNode>,
    ) -> wasmtime::Result<ChannelInterpretation> {
        self.with_node(&node, |node| node.channel_interpretation().into())
    }

    fn set_channel_interpretation(
        &mut self,
        node: Resource<AnyNode>,
        interpretation: ChannelInterpretation,
    ) -> wasmtime::Result<()> {
        self.with_node(&node, |node| {
            node.set_channel_interpretation(interpretation.into())
        })
    }

    fn drop(&mut self, node: Resource<AnyNode>) -> wasmtime::Result<()> {
        self.table.delete(node)?;
        Ok(())
    }
}

impl HostAudioDestinationNode for AudioCtx<'_> {
    fn node(
        &mut self,
        destination: Resource<Node<node::AudioDestinationNode>>,
    ) -> wasmtime::Result<Resource<AnyNode>> {
        let any = self.table.get(&destination)?.any();
        self.handle(any)
    }

    fn max_channel_count(
        &mut self,
        destination: Resource<Node<node::AudioDestinationNode>>,
    ) -> wasmtime::Result<u32> {
        Ok(self
            .table
            .get(&destination)?
            .with(|destination| destination.max_channel_count() as u32))
    }

    fn drop(
        &mut self,
        destination: Resource<Node<node::AudioDestinationNode>>,
    ) -> wasmtime::Result<()> {
        self.table.delete(destination)?;
        Ok(())
    }
}

impl HostAudioParam for AudioCtx<'_> {
    fn value(&mut self, param: Resource<AudioParam>) -> wasmtime::Result<f32> {
        Ok(self.param(&param)?.value())
    }

    fn set_value(&mut self, param: Resource<AudioParam>, value: f32) -> wasmtime::Result<()> {
        let param = self.param(&param)?;
        trapping(|| {
            param.set_value(value);
        })
    }

    fn automation_rate(&mut self, param: Resource<AudioParam>) -> wasmtime::Result<AutomationRate> {
        let param = self.param(&param)?;
        trapping(|| param.automation_rate().into())
    }

    fn set_automation_rate(
        &mut self,
        param: Resource<AudioParam>,
        rate: AutomationRate,
    ) -> wasmtime::Result<()> {
        let param = self.param(&param)?;
        trapping(|| param.set_automation_rate(rate.into()))
    }

    fn default_value(&mut self, param: Resource<AudioParam>) -> wasmtime::Result<f32> {
        Ok(self.param(&param)?.default_value())
    }

    fn min_value(&mut self, param: Resource<AudioParam>) -> wasmtime::Result<f32> {
        Ok(self.param(&param)?.min_value())
    }

    fn max_value(&mut self, param: Resource<AudioParam>) -> wasmtime::Result<f32> {
        Ok(self.param(&param)?.max_value())
    }

    fn set_value_at_time(
        &mut self,
        param: Resource<AudioParam>,
        value: f32,
        start_time: f64,
    ) -> wasmtime::Result<()> {
        let param = self.param(&param)?;
        trapping(|| {
            param.set_value_at_time(value, start_time);
        })
    }

    fn linear_ramp_to_value_at_time(
        &mut self,
        param: Resource<AudioParam>,
        value: f32,
        end_time: f64,
    ) -> wasmtime::Result<()> {
        let param = self.param(&param)?;
        trapping(|| {
            param.linear_ramp_to_value_at_time(value, end_time);
        })
    }

    fn exponential_ramp_to_value_at_time(
        &mut self,
        param: Resource<AudioParam>,
        value: f32,
        end_time: f64,
    ) -> wasmtime::Result<()> {
        let param = self.param(&param)?;
        trapping(|| {
            param.exponential_ramp_to_value_at_time(value, end_time);
        })
    }

    fn set_target_at_time(
        &mut self,
        param: Resource<AudioParam>,
        target: f32,
        start_time: f64,
        time_constant: f32,
    ) -> wasmtime::Result<()> {
        let param = self.param(&param)?;
        trapping(|| {
            param.set_target_at_time(target, start_time, f64::from(time_constant));
        })
    }

    fn set_value_curve_at_time(
        &mut self,
        param: Resource<AudioParam>,
        values: Vec<f32>,
        start_time: f64,
        duration: f64,
    ) -> wasmtime::Result<()> {
        let param = self.param(&param)?;
        trapping(|| {
            param.set_value_curve_at_time(&values, start_time, duration);
        })
    }

    fn cancel_scheduled_values(
        &mut self,
        param: Resource<AudioParam>,
        cancel_time: f64,
    ) -> wasmtime::Result<()> {
        let param = self.param(&param)?;
        trapping(|| {
            param.cancel_scheduled_values(cancel_time);
        })
    }

    fn cancel_and_hold_at_time(
        &mut self,
        param: Resource<AudioParam>,
        cancel_time: f64,
    ) -> wasmtime::Result<()> {
        let param = self.param(&param)?;
        trapping(|| {
            param.cancel_and_hold_at_time(cancel_time);
        })
    }

    fn drop(&mut self, param: Resource<AudioParam>) -> wasmtime::Result<()> {
        self.table.delete(param)?;
        Ok(())
    }
}

impl HostAudioBuffer for AudioCtx<'_> {
    fn sample_rate(&mut self, buffer: Resource<AudioBuffer>) -> wasmtime::Result<f32> {
        Ok(self.table.get(&buffer)?.sample_rate())
    }

    fn length(&mut self, buffer: Resource<AudioBuffer>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&buffer)?.length() as u32)
    }

    fn duration(&mut self, buffer: Resource<AudioBuffer>) -> wasmtime::Result<f64> {
        Ok(self.table.get(&buffer)?.duration())
    }

    fn number_of_channels(&mut self, buffer: Resource<AudioBuffer>) -> wasmtime::Result<u32> {
        Ok(self.table.get(&buffer)?.number_of_channels() as u32)
    }

    fn get_channel_data(
        &mut self,
        buffer: Resource<AudioBuffer>,
        channel: u32,
    ) -> wasmtime::Result<Vec<f32>> {
        let buffer = self.table.get(&buffer)?;
        trapping(|| buffer.get_channel_data(channel as usize).to_vec())
    }

    fn copy_from_channel(
        &mut self,
        buffer: Resource<AudioBuffer>,
        channel: u32,
        start: u32,
        length: u32,
    ) -> wasmtime::Result<Vec<f32>> {
        let buffer = self.table.get(&buffer)?;
        let available = buffer.length().saturating_sub(start as usize);
        let mut samples = vec![0.0; (length as usize).min(available)];
        trapping(|| {
            buffer.copy_from_channel_with_offset(&mut samples, channel as usize, start as usize);
        })?;
        Ok(samples)
    }

    fn copy_to_channel(
        &mut self,
        buffer: Resource<AudioBuffer>,
        samples: Vec<f32>,
        channel: u32,
        start: u32,
    ) -> wasmtime::Result<()> {
        let buffer = self.table.get_mut(&buffer)?;
        trapping(|| buffer.copy_to_channel_with_offset(&samples, channel as usize, start as usize))
    }

    fn drop(&mut self, buffer: Resource<AudioBuffer>) -> wasmtime::Result<()> {
        self.table.delete(buffer)?;
        Ok(())
    }
}

impl HostPeriodicWave for AudioCtx<'_> {
    fn drop(&mut self, wave: Resource<PeriodicWave>) -> wasmtime::Result<()> {
        self.table.delete(wave)?;
        Ok(())
    }
}

impl HostAudioListener for AudioCtx<'_> {
    fn position_x(
        &mut self,
        listener: Resource<AudioListener>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.table.get(&listener)?.position_x().clone();
        self.handle(param)
    }

    fn position_y(
        &mut self,
        listener: Resource<AudioListener>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.table.get(&listener)?.position_y().clone();
        self.handle(param)
    }

    fn position_z(
        &mut self,
        listener: Resource<AudioListener>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.table.get(&listener)?.position_z().clone();
        self.handle(param)
    }

    fn forward_x(
        &mut self,
        listener: Resource<AudioListener>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.table.get(&listener)?.forward_x().clone();
        self.handle(param)
    }

    fn forward_y(
        &mut self,
        listener: Resource<AudioListener>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.table.get(&listener)?.forward_y().clone();
        self.handle(param)
    }

    fn forward_z(
        &mut self,
        listener: Resource<AudioListener>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.table.get(&listener)?.forward_z().clone();
        self.handle(param)
    }

    fn up_x(
        &mut self,
        listener: Resource<AudioListener>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.table.get(&listener)?.up_x().clone();
        self.handle(param)
    }

    fn up_y(
        &mut self,
        listener: Resource<AudioListener>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.table.get(&listener)?.up_y().clone();
        self.handle(param)
    }

    fn up_z(
        &mut self,
        listener: Resource<AudioListener>,
    ) -> wasmtime::Result<Resource<AudioParam>> {
        let param = self.table.get(&listener)?.up_z().clone();
        self.handle(param)
    }

    fn drop(&mut self, listener: Resource<AudioListener>) -> wasmtime::Result<()> {
        self.table.delete(listener)?;
        Ok(())
    }
}
