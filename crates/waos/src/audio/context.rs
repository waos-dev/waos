//! A program's audio context: web-audio-api's, with the output the program set it to, what of its
//! sound keeps the program thawed, and the changes of its state; suspended, resumed and closed as
//! the program asks, and closed once the program lets go of it, its base among its handles.
//!
//! The invariant: a context is closed when its last holder drops it, since web-audio-api keeps a
//! context that is running playing after its last handle is gone; the close waits for the render
//! thread, so it runs on a thread of its own, and a device that hangs holds nobody.

use std::sync::{Arc, Mutex};
use std::thread;

use futures::stream::BoxStream;
use waos_capabilities::web_audio_api::context::{
    AudioContext, AudioContextState, BaseAudioContext,
};

use super::base_context::StateChanges;
use super::closed;
use super::mistakes::failing_async;
use super::sound::ContextSound;
use crate::bindings::wasdk::platform::audio::AudioError;
use crate::sync::locked;

/// The value behind a program's `audio-context`. Every clone is the same context.
#[derive(Clone)]
pub struct Context(Arc<ContextParts>);

struct ContextParts {
    context: Arc<AudioContext>,
    /// The output as the program named it: an id the user picked, or empty for the default.
    sink_id: Mutex<String>,
    sound: Arc<ContextSound>,
    state_changes: StateChanges,
}

impl Context {
    /// `context`, made on the output `sink_id` names, whose sound is `sound`.
    pub(super) fn new(context: AudioContext, sink_id: String, sound: Arc<ContextSound>) -> Context {
        let state_changes = StateChanges::of(context.base());
        Context(Arc::new(ContextParts {
            context: Arc::new(context),
            sink_id: Mutex::new(sink_id),
            sound,
            state_changes,
        }))
    }

    pub(super) fn context(&self) -> &AudioContext {
        &self.0.context
    }

    pub(super) fn sound(&self) -> &Arc<ContextSound> {
        &self.0.sound
    }

    pub(super) fn sink_id(&self) -> String {
        locked(&self.0.sink_id).clone()
    }

    pub(super) fn set_sink_id(&self, sink_id: String) {
        *locked(&self.0.sink_id) = sink_id;
    }

    /// Each state the context changes to from now on.
    pub(super) fn state_changes(&self) -> BoxStream<'static, AudioContextState> {
        self.0.state_changes.stream()
    }

    /// The context, shared with a call that outlasts the program's: web-audio-api's own
    /// suspending, resuming and moving.
    pub(super) fn shared(&self) -> Arc<AudioContext> {
        Arc::clone(&self.0.context)
    }
}

impl Context {
    /// Stops its clock and its output.
    pub(super) async fn suspend(&self) -> Result<(), AudioError> {
        if self.context().state() == AudioContextState::Closed {
            return Err(closed());
        }
        let shared = self.shared();
        failing_async(async move { shared.suspend().await })
            .await
            .map_err(AudioError::InvalidState)?;
        self.sound().set_running(false);
        Ok(())
    }

    /// Starts its clock and its output again.
    pub(super) async fn resume(&self) -> Result<(), AudioError> {
        if self.context().state() == AudioContextState::Closed {
            return Err(closed());
        }
        let shared = self.shared();
        failing_async(async move { shared.resume().await })
            .await
            .map_err(AudioError::InvalidState)?;
        self.sound().set_running(true);
        Ok(())
    }

    /// Ends it for good. A device that fails as it closes leaves it closed all the same.
    pub(super) async fn close(&self) {
        self.sound().set_running(false);
        let shared = self.shared();
        if let Err(failure) = failing_async(async move { shared.close().await }).await {
            tracing::debug!("an audio context's device failed as it closed: {failure}");
        }
    }
}

impl Drop for ContextParts {
    fn drop(&mut self) {
        self.sound.set_running(false);
        let context = Arc::clone(&self.context);
        let closing = thread::Builder::new()
            .name("closing an audio context".into())
            .spawn(move || context.close_sync());
        if let Err(error) = closing {
            tracing::warn!("an audio context was left open, as no thread closes it: {error}");
        }
    }
}
