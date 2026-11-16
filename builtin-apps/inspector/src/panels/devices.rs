//! A program's devices as it uses them: the camera and the microphone it opens and captures from,
//! the machines its user picked for it and the commands it runs on them; a row per event, what it
//! used, what it did, how long where it ended, when and how WaOS decided it.
//!
//! The invariant: an ending is a row of its own, since the events of an ending do not say which
//! start they end where several run at once.

use wasdk::inspect::trace::{DeviceEvent, DeviceUse, MachineEvent, MachineUse};

use super::{Column, Decided, Kept, Panel, PanelRow};
use crate::{clock, words};

const COLUMNS: [Column; 5] = [
    ("Name", 0.0),
    ("Kind", 116.0),
    ("Time", 72.0),
    ("Started", 76.0),
    ("Decided", 72.0),
];

/// Which of the devices a camera's or the microphone's event is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capturing {
    Camera,
    Microphone,
}

/// The rows heard so far.
#[derive(Debug, Default)]
pub(crate) struct DeviceLog {
    rows: Kept<DeviceRow>,
}

impl DeviceLog {
    pub(crate) fn add_capture(&mut self, device: Capturing, event: &DeviceEvent) {
        let name = match device {
            Capturing::Camera => words::CAMERA,
            Capturing::Microphone => words::MICROPHONE,
        };
        let (kind, name, took) = match &event.kind {
            DeviceUse::Opened(opened) => {
                let mut said = name.to_owned();
                if let Some(id) = &opened.camera_id {
                    said = format!("{said} {id}");
                }
                if opened.simulated {
                    said = format!("{said}, {}", words::WAOS_OWN);
                }
                ("Opened", said, None)
            }
            DeviceUse::CaptureStarted => ("Capture started", name.to_owned(), None),
            DeviceUse::CaptureEnded(took) => ("Capture ended", name.to_owned(), Some(*took)),
        };
        self.rows.push(DeviceRow {
            at: event.at.seconds,
            kind,
            name,
            took,
            decided: Decided::of(&event.verdict),
        });
    }

    pub(crate) fn add_machine(&mut self, event: &MachineEvent) {
        let (kind, name, took) = match &event.kind {
            MachineUse::MachinePicked(id) => ("Machine picked", id.clone(), None),
            MachineUse::CommandStarted(started) => {
                let mut said = std::iter::once(started.command.as_str())
                    .chain(started.arguments.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                said = format!("{said}, on {}", started.machine);
                if started.in_terminal {
                    said = format!("{said}, {}", words::IN_A_TERMINAL);
                }
                ("Command started", said, None)
            }
            MachineUse::CommandEnded(took) => {
                ("Command ended", words::A_COMMAND.to_owned(), Some(*took))
            }
        };
        self.rows.push(DeviceRow {
            at: event.at.seconds,
            kind,
            name,
            took,
            decided: Decided::of(&event.verdict),
        });
    }
}

impl Panel for DeviceLog {
    fn columns(&self) -> &'static [Column] {
        &COLUMNS
    }

    fn rows(&self) -> Vec<PanelRow> {
        self.rows.iter().map(DeviceRow::drawn).collect()
    }

    fn nothing_yet(&self) -> &'static str {
        words::NO_DEVICES_YET
    }
}

#[derive(Clone, Debug, PartialEq)]
struct DeviceRow {
    at: i64,
    kind: &'static str,
    name: String,
    /// How long it lasted, in nanoseconds, for an ending.
    took: Option<u64>,
    decided: Decided,
}

impl DeviceRow {
    fn drawn(&self) -> PanelRow {
        let took = self.took.map(words::took);
        let (started, decided) = (
            clock::time_of_day(self.at),
            words::decided(&self.decided).to_owned(),
        );
        let mut details = vec![started.clone()];
        details.extend(took.clone());
        details.push(decided.clone());
        PanelRow {
            cells: vec![
                self.name.clone(),
                self.kind.to_owned(),
                took.unwrap_or_default(),
                started,
                decided,
            ],
            title: format!("{}: {}", self.kind, self.name),
            details,
            beneath: words::why_decided(&self.decided).into_iter().collect(),
        }
    }
}
