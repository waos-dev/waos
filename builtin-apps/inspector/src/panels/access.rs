//! What a program reaches that its user decides, as it reaches it, grouped as iOS's "Data & Sensor
//! Access" groups an app's: the camera and the microphone it opens and captures from, the
//! clipboard it reads and writes, the links it opens, the notifications it posts, the questions it
//! puts to its user and their answers, and the machines its user picked for it and the commands it
//! runs on them. A row per event: what it reached, what it did, how long where it ended, when, and
//! how WaOS decided it, where WaOS decided it.
//!
//! The invariant: an ending is a row of its own, since the events of an ending do not say which
//! start they end where several run at once; and an event WaOS did not decide, a question asked
//! or answered, says no decision.

use wasdk::inspect::trace::{
    ClipboardEvent, ClipboardUse, DeviceEvent, DeviceUse, LinkEvent, MachineEvent, MachineUse,
    NotificationEvent, PermissionEvent, PermissionUse,
};

use super::{Column, Decided, Kept, Panel, PanelRow};
use crate::{clock, words};

const COLUMNS: [Column; 5] = [
    ("Name", 0.0),
    ("Kind", 132.0),
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
pub(crate) struct AccessLog {
    rows: Kept<AccessRow>,
}

impl AccessLog {
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
        self.add(event.at.seconds, (kind, name, took), Some(&event.verdict));
    }

    pub(crate) fn add_clipboard(&mut self, event: &ClipboardEvent) {
        let (kind, name) = match &event.kind {
            ClipboardUse::Read(read) if read.pasted => ("Pasted", read.form.clone()),
            ClipboardUse::Read(read) => ("Clipboard read", read.form.clone()),
            ClipboardUse::Write(forms) => ("Clipboard written", forms.join(", ")),
        };
        self.add(event.at.seconds, (kind, name, None), Some(&event.verdict));
    }

    pub(crate) fn add_link(&mut self, event: &LinkEvent) {
        let said = ("Link opened", event.link.clone(), None);
        self.add(event.at.seconds, said, Some(&event.verdict));
    }

    pub(crate) fn add_notification(&mut self, event: &NotificationEvent) {
        let said = ("Notification", words::A_NOTIFICATION.to_owned(), None);
        self.add(event.at.seconds, said, Some(&event.verdict));
    }

    pub(crate) fn add_permission(&mut self, event: &PermissionEvent) {
        let (kind, name) = match &event.kind {
            PermissionUse::QuestionAsked(question) => ("Asked", words::question(question)),
            PermissionUse::QuestionAnswered(answered) => (
                "Answered",
                format!(
                    "{}: {}",
                    words::question(&answered.question),
                    words::answer(answered.answer.as_deref())
                ),
            ),
            PermissionUse::SettingsOpened(switch) => ("Settings opened", words::question(switch)),
            PermissionUse::PickMounted(path) => ("Pick mounted", path.clone()),
        };
        self.add(event.at.seconds, (kind, name, None), None);
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
        self.add(event.at.seconds, (kind, name, took), Some(&event.verdict));
    }

    fn add(
        &mut self,
        at: i64,
        (kind, name, took): (&'static str, String, Option<u64>),
        verdict: Option<&wasdk::inspect::trace::Verdict>,
    ) {
        self.rows.push(AccessRow {
            at,
            kind,
            name,
            took,
            decided: verdict.map(Decided::of),
        });
    }
}

impl Panel for AccessLog {
    fn columns(&self) -> &'static [Column] {
        &COLUMNS
    }

    fn rows(&self) -> Vec<PanelRow> {
        self.rows.iter().map(AccessRow::drawn).collect()
    }

    fn nothing_yet(&self) -> &'static str {
        words::NO_ACCESS_YET
    }
}

#[derive(Clone, Debug, PartialEq)]
struct AccessRow {
    at: i64,
    kind: &'static str,
    name: String,
    /// How long it lasted, in nanoseconds, for an ending.
    took: Option<u64>,
    /// How WaOS decided it; none for what it did not decide.
    decided: Option<Decided>,
}

impl AccessRow {
    fn drawn(&self) -> PanelRow {
        let took = self.took.map(words::took);
        let started = clock::time_of_day(self.at);
        let decided = self
            .decided
            .as_ref()
            .map(|decided| words::decided(decided).to_owned());
        let mut details = vec![started.clone()];
        details.extend(took.clone());
        details.extend(decided.clone());
        PanelRow {
            cells: vec![
                self.name.clone(),
                self.kind.to_owned(),
                took.unwrap_or_default(),
                started,
                decided.unwrap_or_default(),
            ],
            title: format!("{}: {}", self.kind, self.name),
            details,
            beneath: self.decided.iter().filter_map(words::why_decided).collect(),
        }
    }
}
