//! The platform's stream input: samples a program writes ahead of its graph, which the graph takes
//! on its render thread a block at a time, as the media stream source it is to web-audio-api, and
//! plays as silence while none wait.
//!
//! The invariant: the render thread never waits on the program. It takes what is queued without
//! waiting, and silence when nothing is; only the program's write waits, and only while what it
//! wrote is a latency ahead, the queue's bound.

use std::error::Error;

use tokio::sync::mpsc::{self, error::TryRecvError};
use waos_capabilities::web_audio_api::AudioBuffer;
use waos_capabilities::web_audio_api::context::{
    AudioContext, AudioContextState, BaseAudioContext,
};
use waos_capabilities::web_audio_api::media_streams::MediaStreamTrack;
use waos_capabilities::web_audio_api::node::{AudioNode, MediaStreamTrackAudioSourceNode};
use wasmtime::ensure;

use super::nodes::{AnyNode, Node};
use super::sound::Playing;

/// How far ahead of what the graph plays a program may write: a tenth of a second.
const LATENCY: f64 = 0.1;

/// The frames the graph takes at once: Web Audio's render quantum.
const BLOCK: usize = 128;

/// The most channels a frame may have: web-audio-api's.
pub(super) const MAX_CHANNELS: u32 = 32;

/// A stream input: the value behind a program's `stream-input-node`. Dropped, the graph plays
/// what was written, then the source ends.
pub struct StreamInput {
    node: Node<MediaStreamTrackAudioSourceNode>,
    queue: mpsc::Sender<AudioBuffer>,
    channels: usize,
    sample_rate: f32,
    /// The stream counts as playing while it is open.
    _playing: Playing,
}

/// Why a write was not taken.
pub(super) enum WriteError {
    /// The context is closed: nothing takes what is written.
    Closed,
}

impl StreamInput {
    /// A stream input of `channels` channels on `context`, playing as `playing` holds. A
    /// count of channels web-audio-api refuses is the program's mistake, and traps.
    pub(super) fn new(
        context: &AudioContext,
        channels: u32,
        playing: Playing,
    ) -> wasmtime::Result<StreamInput> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "NotSupportedError - a stream input of {channels} channels: 1 to {MAX_CHANNELS}"
        );
        let channels = channels as usize;
        let sample_rate = context.sample_rate();
        let (queue, taken) = mpsc::channel(blocks_in(LATENCY, sample_rate));
        let silence = AudioBuffer::from(vec![vec![0.0; BLOCK]; channels], sample_rate);
        let track = MediaStreamTrack::from_iter(Taken { taken, silence });
        let node = context.create_media_stream_track_source(&track);
        Ok(StreamInput {
            node: Node::new(node),
            queue,
            channels,
            sample_rate,
            _playing: playing,
        })
    }

    pub(super) fn any(&self) -> AnyNode {
        self.node.any()
    }

    pub(super) fn channels(&self) -> u32 {
        self.channels as u32
    }

    pub(super) fn latency(&self) -> f64 {
        LATENCY
    }

    /// The blocks `samples` make, frames of the stream's channels interleaved, ready to queue. A
    /// count of samples that is no whole number of frames is the program's mistake, and traps.
    pub(super) fn blocks_of(&self, samples: &[f32]) -> wasmtime::Result<Vec<AudioBuffer>> {
        ensure!(
            samples.len().is_multiple_of(self.channels),
            "IndexSizeError - {} samples are no whole number of frames of {} channels",
            samples.len(),
            self.channels
        );
        Ok(samples
            .chunks(BLOCK * self.channels)
            .map(|block| AudioBuffer::from(deinterleaved(block, self.channels), self.sample_rate))
            .collect())
    }

    /// What queues `blocks`, waiting while the queue is full, from the program's call.
    pub(super) fn writer(&self) -> Writer {
        Writer {
            queue: self.queue.clone(),
            context_closed: self
                .node
                .with(|node| node.context().state() == AudioContextState::Closed),
        }
    }
}

/// A write on its way into the queue.
pub(super) struct Writer {
    queue: mpsc::Sender<AudioBuffer>,
    context_closed: bool,
}

impl Writer {
    /// Queues `blocks` in order, each once there is room; fails once nothing takes them.
    pub(super) async fn write(self, blocks: Vec<AudioBuffer>) -> Result<(), WriteError> {
        if self.context_closed {
            return Err(WriteError::Closed);
        }
        for block in blocks {
            self.queue
                .send(block)
                .await
                .map_err(|_| WriteError::Closed)?;
        }
        Ok(())
    }
}

/// The blocks of `sample_rate` frames a second that `latency` seconds hold, one at least.
fn blocks_in(latency: f64, sample_rate: f32) -> usize {
    let frames = latency * f64::from(sample_rate);
    ((frames / BLOCK as f64).ceil() as usize).max(1)
}

/// `block`'s frames of `channels` interleaved samples, as one list per channel.
fn deinterleaved(block: &[f32], channels: usize) -> Vec<Vec<f32>> {
    (0..channels)
        .map(|channel| {
            block
                .iter()
                .skip(channel)
                .step_by(channels)
                .copied()
                .collect()
        })
        .collect()
}

/// What the render thread takes from: the queue, and silence while it is empty.
struct Taken {
    taken: mpsc::Receiver<AudioBuffer>,
    silence: AudioBuffer,
}

impl Iterator for Taken {
    type Item = Result<AudioBuffer, Box<dyn Error + Send + Sync>>;

    /// The next block written, silence while none is, and the end once the program let go of
    /// the stream and everything written is played.
    fn next(&mut self) -> Option<Self::Item> {
        match self.taken.try_recv() {
            Ok(block) => Some(Ok(block)),
            Err(TryRecvError::Empty) => Some(Ok(self.silence.clone())),
            Err(TryRecvError::Disconnected) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_latency_holds_whole_blocks_and_one_at_least() {
        assert_eq!(blocks_in(0.1, 48_000.0), 38);
        assert_eq!(blocks_in(0.0, 48_000.0), 1);
    }

    #[test]
    fn frames_are_deinterleaved_by_channel() {
        assert_eq!(
            deinterleaved(&[1.0, -1.0, 2.0, -2.0], 2),
            [vec![1.0, 2.0], vec![-1.0, -2.0]]
        );
    }

    #[test]
    fn the_render_thread_takes_what_is_written_then_silence_then_the_end() {
        let (queue, taken) = mpsc::channel(4);
        let silence = AudioBuffer::from(vec![vec![0.0; BLOCK]], 48_000.0);
        let mut taken = Taken { taken, silence };
        queue
            .try_send(AudioBuffer::from(vec![vec![0.5; 3]], 48_000.0))
            .unwrap();
        let first = taken.next().unwrap().unwrap();
        assert_eq!(first.get_channel_data(0), [0.5; 3]);
        let second = taken.next().unwrap().unwrap();
        assert_eq!(second.length(), BLOCK, "silence while nothing waits");
        drop(queue);
        assert!(taken.next().is_none(), "the program let go of it");
    }
}
