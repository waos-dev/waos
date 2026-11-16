//! What the detail pane says of the row selected, whichever panel it is of, as DevTools' pane says
//! of a request: what it was, why WaOS refused it where it did, and its facts in sections; and the
//! glyph each kind of thing is drawn with, which each kit maps to its own icons.
//!
//! The invariant: a detail says only what its row says, so the pane never reaches back to the
//! events or the device.

use crate::logic::panels::{Decided, Tone};
use crate::words;

/// What a row, a capability or a use is drawn with, named by what it is, not by any kit's icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Glyph {
    Request,
    Lookup,
    Connection,
    Listener,
    Datagram,
    File,
    Folder,
    Watch,
    Camera,
    Microphone,
    Clipboard,
    Link,
    Notification,
    Question,
    Machine,
    Output,
}

impl Glyph {
    /// The glyph each kind of access is drawn with.
    pub(crate) fn of_access(kind: waos_activity::AccessKind) -> Glyph {
        use waos_activity::AccessKind;
        match kind {
            AccessKind::Camera => Glyph::Camera,
            AccessKind::Microphone => Glyph::Microphone,
            AccessKind::Network => Glyph::Request,
            AccessKind::Files => Glyph::Folder,
            AccessKind::Clipboard => Glyph::Clipboard,
            AccessKind::Machines => Glyph::Machine,
            AccessKind::Links => Glyph::Link,
            AccessKind::Notifications => Glyph::Notification,
            AccessKind::Questions => Glyph::Question,
        }
    }
}

/// What the detail pane says of a row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Detail {
    pub(crate) glyph: Glyph,
    /// What it was: `Request refused`, `Capture, 2 min 14 s`.
    pub(crate) title: String,
    /// What it reached: `GET https://www.nasa.gov/feed/`.
    pub(crate) subtitle: String,
    /// Why WaOS refused it, where it did.
    pub(crate) refusal: Option<Refusal>,
    pub(crate) sections: Vec<Section>,
}

/// Why WaOS refused a use, and where its user changes that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    /// The key of the setting that refused it: `network`.
    pub(crate) setting: String,
    pub(crate) said: String,
}

/// A section of the pane: its title and its facts.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Section {
    pub(crate) title: &'static str,
    pub(crate) fields: Vec<Field>,
}

/// A fact: what it is, and its value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    pub(crate) label: &'static str,
    pub(crate) value: String,
    pub(crate) tone: Tone,
    /// Whether the value is a URL, a path, an address or a time, which read best monospaced.
    pub(crate) code: bool,
}

impl Field {
    pub(crate) fn plain(label: &'static str, value: impl Into<String>) -> Field {
        Field {
            label,
            value: value.into(),
            tone: Tone::Plain,
            code: false,
        }
    }

    pub(crate) fn code(label: &'static str, value: impl Into<String>) -> Field {
        Field {
            label,
            value: value.into(),
            tone: Tone::Plain,
            code: true,
        }
    }
}

impl Refusal {
    /// Why `decided` refused, where it did.
    pub(crate) fn of(decided: &Decided) -> Option<Refusal> {
        match decided {
            Decided::Refused(setting) => Some(Refusal {
                setting: setting.clone(),
                said: words::refused_by(setting),
            }),
            Decided::Allowed | Decided::Failed => None,
        }
    }
}

/// The section saying how WaOS decided a use, where it decided it.
pub(crate) fn decision(decided: &Decided) -> Section {
    let mut fields = vec![Field {
        label: words::DECIDED,
        value: words::decided(decided).to_owned(),
        tone: if decided.refused() {
            Tone::Refused
        } else {
            Tone::Allowed
        },
        code: false,
    }];
    if let Decided::Refused(setting) = decided {
        fields.push(Field::code(words::SETTING, setting.clone()));
    }
    Section {
        title: words::DECISION,
        fields,
    }
}
