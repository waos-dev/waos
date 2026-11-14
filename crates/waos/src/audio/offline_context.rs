//! A program's offline context: web-audio-api's, a graph rendered on no output, as fast as it
//! goes, into a buffer of the length the context was made with, once, and the changes of its
//! state.
//!
//! The invariant: rendering runs on a thread of the runtime's blocking pool, never the program's,
//! since it takes as long as the graph's work does; it goes on while the program is frozen, as a
//! context's scheduled sound plays out, and what it rendered waits for the program.

use std::sync::Arc;

use futures::executor::block_on;
use futures::stream::BoxStream;
use waos_capabilities::web_audio_api::AudioBuffer;
use waos_capabilities::web_audio_api::context::{
    AudioContextState, BaseAudioContext, OfflineAudioContext,
};
use wasmtime::ensure;

use super::base_context::StateChanges;
use super::mistakes::trapping;
use super::{MOST_SAMPLES, off_the_program_thread};

/// The value behind a program's `offline-audio-context`. Every clone is the same context.
#[derive(Clone)]
pub struct OfflineContext(Arc<OfflineParts>);

struct OfflineParts {
    context: Arc<OfflineAudioContext>,
    state_changes: StateChanges,
}

impl OfflineContext {
    /// A context rendering `length` frames of `channels` channels at `sample_rate`. One of more
    /// samples than a buffer may hold, or one web-audio-api refuses, is the program's mistake,
    /// and traps.
    pub(super) fn new(
        channels: u32,
        length: u32,
        sample_rate: f32,
    ) -> wasmtime::Result<OfflineContext> {
        let samples = u64::from(channels) * u64::from(length);
        ensure!(
            samples <= MOST_SAMPLES,
            "NotSupportedError - an offline context of {channels} channels of {length} frames"
        );
        let context =
            trapping(|| OfflineAudioContext::new(channels as usize, length as usize, sample_rate))?;
        let state_changes = StateChanges::of(context.base());
        Ok(OfflineContext(Arc::new(OfflineParts {
            context: Arc::new(context),
            state_changes,
        })))
    }

    pub(super) fn context(&self) -> &OfflineAudioContext {
        &self.0.context
    }

    /// How many frames it renders.
    pub(super) fn length(&self) -> u32 {
        // Made from a `u32`.
        self.0.context.length() as u32
    }

    /// Each state the context changes to from now on.
    pub(super) fn state_changes(&self) -> BoxStream<'static, AudioContextState> {
        self.0.state_changes.stream()
    }

    /// The graph rendered, off the program's thread. Rendering twice is the program's mistake,
    /// and traps.
    pub(super) async fn render(&self) -> wasmtime::Result<AudioBuffer> {
        let context = Arc::clone(&self.0.context);
        off_the_program_thread(move || trapping(|| block_on(context.start_rendering()))).await?
    }
}

#[cfg(test)]
mod tests {
    use waos_capabilities::web_audio_api::node::{AudioNode, AudioScheduledSourceNode};

    use super::*;

    fn rendered(context: &OfflineContext) -> wasmtime::Result<AudioBuffer> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(context.render())
    }

    #[test]
    fn a_graph_renders_into_a_buffer_of_the_context_s_length() {
        let context = OfflineContext::new(1, 256, 48_000.0).unwrap();
        let base = context.context();
        let mut source = base.create_constant_source();
        source.offset().set_value(0.5);
        let gain = base.create_gain();
        gain.gain().set_value(0.5);
        source.connect(&gain);
        gain.connect(&base.destination());
        source.start();
        let buffer = rendered(&context).unwrap();
        assert_eq!(
            (buffer.number_of_channels(), buffer.length()),
            (1, context.length() as usize)
        );
        assert_eq!(buffer.get_channel_data(0), [0.25; 256]);
        assert_eq!(base.state(), AudioContextState::Closed);
    }

    #[test]
    fn rendering_twice_traps() {
        let context = OfflineContext::new(1, 128, 48_000.0).unwrap();
        rendered(&context).unwrap();
        let trap = rendered(&context).expect_err("a context renders once");
        assert!(
            trap.to_string()
                .starts_with("InvalidStateError - Cannot call `startRendering` twice"),
            "{trap}"
        );
    }

    #[test]
    fn a_context_of_more_samples_than_a_buffer_holds_traps_before_anything_is_made() {
        let trap = OfflineContext::new(2, 1 << 30, 48_000.0)
            .err()
            .expect("more samples than a buffer holds");
        assert!(trap.to_string().starts_with("NotSupportedError"), "{trap}");
        let trap = OfflineContext::new(0, 128, 48_000.0)
            .err()
            .expect("no channels");
        assert!(trap.to_string().starts_with("NotSupportedError"), "{trap}");
    }
}
