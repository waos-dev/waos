//! What a program records with, as the shell's indicator shows it: for each kind of device, the
//! microphone or the camera, how many of that kind the program holds open, the program recording
//! with that kind from its first one opened until its last is let go, the program ending among the
//! ways.
//!
//! The invariant: the shell hears that a program records with a kind of device once, as its first
//! of that kind opens, and that it stopped once, as its last is let go.

use std::sync::{Arc, Mutex};

use crate::running::RunningProgram;
use crate::shell::{RecordingDevice, ShellRecordingIndicator};
use crate::sync::locked;

/// A program's devices of one kind open, as the shell shows them. Every clone counts the same ones.
#[derive(Clone)]
pub(crate) struct Recordings(Arc<RecordingsParts>);

struct RecordingsParts {
    indicator: Option<Arc<dyn ShellRecordingIndicator>>,
    program: RunningProgram,
    device: RecordingDevice,
    open: Mutex<u32>,
}

impl Recordings {
    /// `program`'s devices of the kind `device`, shown by `indicator`; with none, nothing is shown.
    pub(crate) fn new(
        indicator: Option<Arc<dyn ShellRecordingIndicator>>,
        program: RunningProgram,
        device: RecordingDevice,
    ) -> Recordings {
        Recordings(Arc::new(RecordingsParts {
            indicator,
            program,
            device,
            open: Mutex::new(0),
        }))
    }

    /// Whether the shell can show that the program records.
    pub(crate) fn can_show(&self) -> bool {
        self.0.indicator.is_some()
    }

    /// The program records with one more of these devices, until the answer is dropped.
    pub(crate) fn record(&self) -> Recording {
        self.change(|open| *open += 1);
        Recording {
            recordings: self.clone(),
        }
    }

    /// Changes the count of devices open, and tells the shell when it reaches or leaves none.
    fn change(&self, change: impl FnOnce(&mut u32)) {
        let parts = &self.0;
        let mut open = locked(&parts.open);
        let was_recording = *open > 0;
        change(&mut open);
        let recording = *open > 0;
        if recording != was_recording
            && let Some(indicator) = &parts.indicator
        {
            indicator.recording_changed(&parts.program, parts.device, recording);
        }
    }
}

/// One device open, held while it is.
pub(crate) struct Recording {
    recordings: Recordings,
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.recordings.change(|open| *open -= 1);
    }
}
