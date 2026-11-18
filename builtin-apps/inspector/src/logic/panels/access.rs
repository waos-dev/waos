//! What a program reaches that its user decides, as it reaches it, grouped as iOS's "Data & Sensor
//! Access" groups an app's: the camera and the microphone it opens and captures from, the
//! clipboard it reads and writes, the links it opens, the notifications it posts, the questions it
//! puts to its user and their answers, and the machines its user picked for it and the commands it
//! runs on them. A row per event: what it was, how long it lasted where it ended, how WaOS decided
//! it, where WaOS decided it, and when.
//!
//! The invariant: an ending is a row of its own, since the events of an ending do not say which
//! start they end where several run at once; and an event WaOS did not decide, a question asked
//! or answered, says no decision.

use wasdk::inspect::trace::{
    ClipboardEvent, ClipboardUse, DeviceEvent, DeviceUse, LinkEvent, MachineEvent, MachineUse,
    NotificationEvent, PermissionEvent, PermissionUse, Verdict,
};
use wasdk::system_clock::Instant;

use super::{Cell, Column, Decided, PanelRow, RowKind, Rows, Said, Tile, Tone, Wording, found_in};
use crate::logic::detail::{self, Detail, Field, Glyph, Refusal, Section};
use crate::words;

/// Which of the devices a camera's or the microphone's event is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Capturing {
    Camera,
    Microphone,
}

/// What the panel hears, row by row.
#[derive(Debug, Default)]
pub(crate) struct AccessLog;

impl AccessLog {
    pub(crate) fn add_capture(
        &mut self,
        rows: &mut impl Rows<AccessRow>,
        device: Capturing,
        event: &DeviceEvent,
    ) {
        let (name, glyph) = match device {
            Capturing::Camera => (words::CAMERA, Glyph::Camera),
            Capturing::Microphone => (words::MICROPHONE, Glyph::Microphone),
        };
        let (what, lasted) = match &event.kind {
            DeviceUse::Opened(opened) => {
                let mut said = format!("{name} opened");
                if let Some(id) = &opened.camera_id {
                    said = format!("{said}: {id}");
                }
                if opened.simulated {
                    said = format!("{said}, {}", words::WAOS_OWN);
                }
                (said, None)
            }
            DeviceUse::CaptureStarted => (format!("{name} capture started"), None),
            DeviceUse::CaptureEnded(took) => (format!("{name} capture ended"), Some(*took)),
        };
        let said = Summary {
            kind: AccessKind::Capture,
            glyph,
            what,
            lasted,
        };
        rows.append(AccessRow::of(
            event.at,
            event.instance,
            said,
            Some(&event.verdict),
        ));
    }

    pub(crate) fn add_clipboard(
        &mut self,
        rows: &mut impl Rows<AccessRow>,
        event: &ClipboardEvent,
    ) {
        let what = match &event.kind {
            ClipboardUse::Read(read) if read.pasted => format!("Pasted {}", read.form),
            ClipboardUse::Read(read) => format!("Clipboard read without a paste: {}", read.form),
            ClipboardUse::Write(forms) => format!("Clipboard written: {}", forms.join(", ")),
        };
        let said = Summary::new(AccessKind::Clipboard, Glyph::Clipboard, what);
        rows.append(AccessRow::of(
            event.at,
            event.instance,
            said,
            Some(&event.verdict),
        ));
    }

    pub(crate) fn add_link(&mut self, rows: &mut impl Rows<AccessRow>, event: &LinkEvent) {
        let said = Summary::new(
            AccessKind::Link,
            Glyph::Link,
            format!("Link opened: {}", event.link),
        );
        rows.append(AccessRow::of(
            event.at,
            event.instance,
            said,
            Some(&event.verdict),
        ));
    }

    pub(crate) fn add_notification(
        &mut self,
        rows: &mut impl Rows<AccessRow>,
        event: &NotificationEvent,
    ) {
        let said = Summary::new(
            AccessKind::Notification,
            Glyph::Notification,
            words::A_NOTIFICATION.to_owned(),
        );
        rows.append(AccessRow::of(
            event.at,
            event.instance,
            said,
            Some(&event.verdict),
        ));
    }

    pub(crate) fn add_permission(
        &mut self,
        rows: &mut impl Rows<AccessRow>,
        event: &PermissionEvent,
    ) {
        let what = match &event.kind {
            PermissionUse::QuestionAsked(question) => {
                format!("Asked for {}", words::question(question))
            }
            PermissionUse::QuestionAnswered(answered) => format!(
                "Answered for {}: {}",
                words::question(&answered.question),
                words::answer(answered.answer.as_deref())
            ),
            PermissionUse::SettingsOpened(switch) => {
                format!("Settings opened at {}", words::question(switch))
            }
            PermissionUse::PickMounted(path) => format!("Pick mounted: {path}"),
        };
        let said = Summary::new(AccessKind::Question, Glyph::Question, what);
        rows.append(AccessRow::of(event.at, event.instance, said, None));
    }

    pub(crate) fn add_machine(&mut self, rows: &mut impl Rows<AccessRow>, event: &MachineEvent) {
        let (what, lasted) = match &event.kind {
            MachineUse::MachinePicked(id) => (format!("Machine picked: {id}"), None),
            MachineUse::CommandStarted(started) => {
                let command = std::iter::once(started.command.as_str())
                    .chain(started.arguments.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut said = format!("Ran {command}, on {}", started.machine);
                if started.in_terminal {
                    said = format!("{said}, {}", words::IN_A_TERMINAL);
                }
                (said, None)
            }
            MachineUse::CommandEnded(took) => (words::A_COMMAND_ENDED.to_owned(), Some(*took)),
        };
        let said = Summary {
            kind: AccessKind::Machine,
            glyph: Glyph::Machine,
            what,
            lasted,
        };
        rows.append(AccessRow::of(
            event.at,
            event.instance,
            said,
            Some(&event.verdict),
        ));
    }
}

/// The kinds of what a program reaches that its user decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessKind {
    Capture,
    Clipboard,
    Link,
    Notification,
    Question,
    Machine,
}

impl RowKind for AccessKind {
    const ALL: &'static [AccessKind] = &[
        AccessKind::Capture,
        AccessKind::Clipboard,
        AccessKind::Link,
        AccessKind::Notification,
        AccessKind::Question,
        AccessKind::Machine,
    ];

    fn name(self) -> &'static str {
        match self {
            AccessKind::Capture => "Camera, mic",
            AccessKind::Clipboard => "Clipboard",
            AccessKind::Link => "Links",
            AccessKind::Notification => "Notifications",
            AccessKind::Question => "Questions",
            AccessKind::Machine => "Machines",
        }
    }
}

/// What an event says, before when and how it was decided.
struct Summary {
    kind: AccessKind,
    glyph: Glyph,
    what: String,
    lasted: Option<u64>,
}

impl Summary {
    fn new(kind: AccessKind, glyph: Glyph, what: String) -> Summary {
        Summary {
            kind,
            glyph,
            what,
            lasted: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AccessRow {
    at: Instant,
    instance: u32,
    kind: AccessKind,
    glyph: Glyph,
    what: String,
    /// How long it lasted, in nanoseconds, for an ending.
    lasted: Option<u64>,
    /// How WaOS decided it; none for what it did not decide.
    decided: Option<Decided>,
}

impl AccessRow {
    fn of(at: Instant, instance: u32, said: Summary, verdict: Option<&Verdict>) -> AccessRow {
        AccessRow {
            at,
            instance,
            kind: said.kind,
            glyph: said.glyph,
            what: said.what,
            lasted: said.lasted,
            decided: verdict.map(Decided::of),
        }
    }

    fn refused_by_waos(&self) -> bool {
        self.decided.as_ref().is_some_and(Decided::refused)
    }
}

impl PanelRow for AccessRow {
    type Kind = AccessKind;

    const COLUMNS: &'static [Column] = &[
        Column::rest("What"),
        Column::numeric("Lasted", 88.0).stepping_out(),
        Column::points("Decided", 160.0),
        Column::numeric("At", 130.0),
    ];

    const NOTHING_YET: &'static str = words::NO_ACCESS_YET;

    fn kind(&self) -> AccessKind {
        self.kind
    }

    fn glyph(&self) -> Glyph {
        self.glyph
    }

    fn cell(&self, index: usize) -> Cell {
        match index {
            0 if self.refused_by_waos() => Cell::plain(self.what.clone()).toned(Tone::Refused),
            0 => Cell::plain(self.what.clone()),
            1 => Cell::code(self.lasted.map(words::took).unwrap_or_default()),
            2 => match &self.decided {
                Some(decided) => decided.cell(),
                None => Cell::plain(words::NOT_DECIDED).toned(Tone::Quiet),
            },
            _ => Cell::code(Said::moment(self.at, Wording::Precise)).toned(Tone::Quiet),
        }
    }

    fn refused(&self) -> bool {
        self.refused_by_waos()
    }

    fn matches(&self, text: &str) -> bool {
        found_in(text, &[&self.what])
    }

    fn tile(&self) -> Tile {
        let mut details = vec![Said::moment(self.at, Wording::TimeOfDay)];
        details.extend(self.lasted.map(|lasted| Said::text(words::took(lasted))));
        details.extend((self.decided.iter()).map(|decided| Said::text(words::decided(decided))));
        Tile {
            code: false,
            title: self.what.clone(),
            details,
            beneath: self.decided.iter().filter_map(words::why_decided).collect(),
        }
    }

    fn detail(&self) -> Detail {
        let mut fields = vec![
            Field::plain(words::INSTANCE, words::instance(self.instance)),
            Field::code(words::AT, Said::moment(self.at, Wording::Precise)),
        ];
        if let Some(lasted) = self.lasted {
            fields.push(Field::code(words::LASTED, words::took(lasted)));
        }
        let mut sections = vec![Section {
            title: words::EVENT,
            fields,
        }];
        sections.extend(self.decided.as_ref().map(detail::decision));
        Detail {
            glyph: self.glyph,
            title: self.kind.name().to_owned(),
            subtitle: self.what.clone(),
            refusal: self.decided.as_ref().and_then(Refusal::of),
            sections,
        }
    }
}
