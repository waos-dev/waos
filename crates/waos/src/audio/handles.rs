//! The value behind each kind of node's handle, named as the WIT names the kind: web-audio-api's
//! node of that kind, held as a node, or as a scheduled source with its end. Named here because
//! the bindings name a resource's value by a path, which cannot carry a type's parameters.

use waos_capabilities::web_audio_api::node;

use super::{Node, Source};

/// `audio-destination-node`.
pub type AudioDestinationNode = Node<node::AudioDestinationNode>;
/// `gain-node`.
pub type GainNode = Node<node::GainNode>;
/// `oscillator-node`.
pub type OscillatorNode = Source<node::OscillatorNode>;
/// `audio-buffer-source-node`.
pub type AudioBufferSourceNode = Source<node::AudioBufferSourceNode>;
/// `constant-source-node`.
pub type ConstantSourceNode = Source<node::ConstantSourceNode>;
/// `delay-node`.
pub type DelayNode = Node<node::DelayNode>;
/// `biquad-filter-node`.
pub type BiquadFilterNode = Node<node::BiquadFilterNode>;
/// `iir-filter-node`.
pub type IirFilterNode = Node<node::IIRFilterNode>;
/// `dynamics-compressor-node`.
pub type DynamicsCompressorNode = Node<node::DynamicsCompressorNode>;
/// `convolver-node`.
pub type ConvolverNode = Node<node::ConvolverNode>;
/// `wave-shaper-node`.
pub type WaveShaperNode = Node<node::WaveShaperNode>;
/// `stereo-panner-node`.
pub type StereoPannerNode = Node<node::StereoPannerNode>;
/// `panner-node`.
pub type PannerNode = Node<node::PannerNode>;
/// `analyser-node`.
pub type AnalyserNode = Node<node::AnalyserNode>;
/// `channel-splitter-node`.
pub type ChannelSplitterNode = Node<node::ChannelSplitterNode>;
/// `channel-merger-node`.
pub type ChannelMergerNode = Node<node::ChannelMergerNode>;
