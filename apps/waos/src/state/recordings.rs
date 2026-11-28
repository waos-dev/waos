//! The programs recording now, and with what, as the engine's indicator of who records tells them
//! (`logic::recording_indicator`): what the library shows in every layout, so its user sees which
//! program uses the microphone or the camera, where the system shows only that WaOS does.
//!
//! The invariant: a program is listed with a device from the moment the engine says it records
//! with it until it says it stopped, which it says once, its end among the ways, so nothing here
//! watches the programs.

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use std::sync::Arc;
use waft::{App, Context, Entity, WeakEntity};
use waos::{RecordingDevice, RunningProgram};
use waos_package_manager::PackageManager;

use crate::logic::program_name::program_name;
use crate::logic::recording_indicator::RecordingChanged;

pub(crate) struct Recordings {
    /// The programs recording, each with what and with its name as its user knows it, in the
    /// order they began.
    recording: Vec<Recording>,
    /// Where an installed program's name is read.
    packages: Arc<PackageManager>,
}

impl Recordings {
    /// The programs recording, which follow each change arriving on `changes`; an installed
    /// program's name is read from `packages`.
    pub(crate) fn listen(
        app: &mut App,
        changes: UnboundedReceiver<RecordingChanged>,
        packages: Arc<PackageManager>,
    ) -> Entity<Recordings> {
        let recordings = app.new_entity(|_| Recordings {
            recording: Vec::new(),
            packages,
        });
        take_changes(app, recordings.downgrade(), changes);
        recordings
    }

    /// The names of the programs recording with `device` now, in the order they began.
    pub(crate) fn names(&self, device: RecordingDevice) -> Vec<String> {
        self.recording
            .iter()
            .filter(|recording| recording.device == device)
            .map(|recording| recording.name.clone())
            .collect()
    }

    fn changed(&mut self, change: RecordingChanged, cx: &mut Context<Recordings>) {
        let RecordingChanged {
            program,
            device,
            recording,
        } = change;
        tracing::info!(
            program = program.name(),
            ?device,
            recording,
            "a program's recording"
        );
        self.recording
            .retain(|listed| listed.program != program || listed.device != device);
        if recording {
            let name = program_name(&self.packages, &program);
            self.recording.push(Recording {
                program,
                device,
                name,
            });
        }
        cx.notify();
    }
}

/// A program recording with a device.
struct Recording {
    program: RunningProgram,
    device: RecordingDevice,
    /// The program's name as its user knows it.
    name: String,
}

/// One task takes every change the engine tells, on the main thread, for as long as the
/// recordings are there to take them.
fn take_changes(
    app: &mut App,
    recordings: WeakEntity<Recordings>,
    mut changes: UnboundedReceiver<RecordingChanged>,
) {
    app.spawn(async move |cx| {
        while let Some(change) = changes.next().await {
            let Some(recordings) = recordings.upgrade() else {
                break;
            };
            cx.update(|app| {
                recordings.update(app, |recordings, cx| recordings.changed(change, cx));
            });
        }
    });
}
