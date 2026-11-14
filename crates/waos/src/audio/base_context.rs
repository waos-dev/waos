//! What every kind of context is, Web Audio's `BaseAudioContext`, as a program's
//! `base-audio-context` reaches it, whichever kind handed it out: one graph, its clock, its state
//! and the changes of it, the nodes it makes, whose sources sound only on a context that plays,
//! and the data it decodes. The changes of a context's state, which every kind tells alike, are
//! [`StateChanges`]'.
//!
//! The invariant: a base holds the context it is, so a context lives while any handle on it does,
//! and what a base makes is made on that context.

use std::sync::Arc;

use futures::stream::{self, BoxStream, StreamExt};
use tokio::sync::watch;
use waos_capabilities::web_audio_api::AudioBuffer;
use waos_capabilities::web_audio_api::context::{
    AudioContextState, BaseAudioContext, ConcreteBaseAudioContext,
};

use super::context::Context;
use super::decoding::decoded;
use super::off_the_program_thread;
use super::offline_context::OfflineContext;
use super::sound::ContextSound;
use crate::bindings::wasdk::platform::audio::AudioError;

/// The value behind a program's `base-audio-context`. Every clone is the same context.
#[derive(Clone)]
pub enum BaseContext {
    /// An `audio-context`'s, which plays.
    Playing(Context),
    /// An `offline-audio-context`'s, which renders into a buffer.
    Rendering(OfflineContext),
}

impl BaseContext {
    /// web-audio-api's context, as any context: what makes its nodes and reads its clock.
    pub(super) fn context(&self) -> &ConcreteBaseAudioContext {
        match self {
            BaseContext::Playing(context) => context.context().base(),
            BaseContext::Rendering(context) => context.context().base(),
        }
    }

    /// What of its sources' sound keeps its program thawed; none where nothing plays aloud.
    pub(super) fn sound(&self) -> Option<&Arc<ContextSound>> {
        match self {
            BaseContext::Playing(context) => Some(context.sound()),
            BaseContext::Rendering(_) => None,
        }
    }

    /// Each state the context changes to from now on.
    pub(super) fn state_changes(&self) -> BoxStream<'static, AudioContextState> {
        match self {
            BaseContext::Playing(context) => context.state_changes(),
            BaseContext::Rendering(context) => context.state_changes(),
        }
    }

    /// `data` decoded into a buffer at the context's sample rate, off the program's thread, unless
    /// it holds more than a buffer may (`decoding`).
    pub(super) async fn decode(
        &self,
        data: Vec<u8>,
    ) -> wasmtime::Result<Result<AudioBuffer, AudioError>> {
        let context = self.context().clone();
        off_the_program_thread(move || decoded(data, &context)).await
    }
}

/// The changes of one context's state, counted as web-audio-api's one `statechange` handler tells
/// them.
pub(super) struct StateChanges {
    changes: watch::Sender<u64>,
    base: ConcreteBaseAudioContext,
}

impl StateChanges {
    /// The changes of the state of `base`'s context from now on, whose handler is set here, once.
    pub(super) fn of(base: &ConcreteBaseAudioContext) -> StateChanges {
        let changes = watch::Sender::new(0);
        let counted = changes.clone();
        base.set_onstatechange(move |_| counted.send_modify(|changes| *changes += 1));
        StateChanges {
            changes,
            base: base.clone(),
        }
    }

    /// Each state the context changes to from now on, on a stream of its own.
    pub(super) fn stream(&self) -> BoxStream<'static, AudioContextState> {
        let watch = StateWatch {
            changes: self.changes.subscribe(),
            last: self.base.state(),
            base: self.base.clone(),
        };
        stream::unfold(watch, StateWatch::next_change).boxed()
    }
}

/// One stream of a context's states: the changes counted, and the state told last.
struct StateWatch {
    changes: watch::Receiver<u64>,
    base: ConcreteBaseAudioContext,
    last: AudioContextState,
}

impl StateWatch {
    /// The next state another than the one told last; the stream ends with the context.
    async fn next_change(mut self) -> Option<(AudioContextState, StateWatch)> {
        loop {
            self.changes.changed().await.ok()?;
            let state = self.base.state();
            if state != self.last {
                self.last = state;
                return Some((state, self));
            }
        }
    }
}
