//! What shows which programs record, and with what, as the engine reaches it from any thread: each
//! change goes to the main thread, where `state::recordings` keeps the programs recording.
//!
//! The invariant: every change the engine tells reaches the main thread, in the order told, while
//! the main thread listens.

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use waos::{AppInstance, RecordingDevice, ShellRecordingIndicator};

/// A program began or stopped recording with a device, on its way to the main thread.
pub(crate) struct RecordingChanged {
    pub(crate) program: AppInstance,
    pub(crate) device: RecordingDevice,
    pub(crate) recording: bool,
}

/// The app's indicator of who records, as the engine reaches it.
pub(crate) struct RecordingIndicatorOnRequest {
    changes: UnboundedSender<RecordingChanged>,
}

impl RecordingIndicatorOnRequest {
    /// The indicator, and where its changes arrive on the main thread.
    pub(crate) fn new() -> (
        RecordingIndicatorOnRequest,
        UnboundedReceiver<RecordingChanged>,
    ) {
        let (changes, changed) = mpsc::unbounded();
        (RecordingIndicatorOnRequest { changes }, changed)
    }
}

impl ShellRecordingIndicator for RecordingIndicatorOnRequest {
    fn recording_changed(&self, program: &AppInstance, device: RecordingDevice, recording: bool) {
        let changed = RecordingChanged {
            program: program.clone(),
            device,
            recording,
        };
        if self.changes.unbounded_send(changed).is_err() {
            tracing::info!(
                program = program.name(),
                ?device,
                recording,
                "WaOS is quitting: not shown"
            );
        }
    }
}
