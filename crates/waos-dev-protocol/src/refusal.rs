//! Why a request was not carried out: a kind an agent can act on, and a message for a person.
//!
//! The invariant: a kind added leaves the ones before it as they were, and a `cargo waos` older
//! than the WaOS it reaches reads a kind it does not know as `Unknown`, never breaking the
//! connection over a refusal it can still show.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::adjacent::{self, Adjacent};
use crate::messages::WindowId;
use crate::settings_rows::Asked;

/// Why a request was not carried out, and the words for its user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Refusal {
    pub kind: RefusalKind,
    /// The same in words, naming the program, window or build a person needs to know.
    pub message: String,
}

impl Refusal {
    pub fn new(kind: RefusalKind, message: impl Into<String>) -> Refusal {
        Refusal {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a refusal refuses, as an agent acts on it; the message says the rest.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "details")]
pub enum RefusalKind {
    /// No run of `cargo waos run` runs the program now: nothing does, or the program running was
    /// opened otherwise, from WaOS's library.
    NoDevRunNow,
    /// No run of `cargo waos run` on this machine runs the program or has run it, so its settings
    /// are none of a tool's.
    NeverDevRun,
    /// The program has no window `window`.
    NoWindow {
        window: WindowId,
    },
    /// The point `x`, `y` is outside the window, which is `width` by `height` physical pixels at
    /// `scale` physical pixels a logical point.
    OutsideWindow {
        width: u32,
        height: u32,
        x: f64,
        y: f64,
        scale: f64,
    },
    /// The program's view numbered `window` is in no window, as a phone's deck shows a tab: its
    /// size is the platform's, which no tool sets.
    InNoWindow {
        window: WindowId,
    },
    /// WaOS asks the program's user this over the program's windows, so the request was not
    /// carried out: the user's answer comes first.
    Asking(Asked),
    /// An input that cannot be given: a key or chord no key has, or text while nothing in the
    /// window is edited.
    BadInput,
    /// The component is not one, or one that does not load or run.
    CannotLoad,
    /// `Open` names this machine's paths, and the connection is not on this machine.
    LocalOnly,
    /// The request itself could not be carried out, as a store that cannot be read or a picture
    /// that cannot be taken: the message says what failed.
    Failed,
    Unknown,
}

impl Adjacent for RefusalKind {
    const TAG: &'static str = "kind";
    const CONTENT: &'static str = "details";

    fn unknown() -> Self {
        RefusalKind::Unknown
    }

    fn is_known(tag: &str) -> bool {
        matches!(
            tag,
            "NoDevRunNow"
                | "NeverDevRun"
                | "NoWindow"
                | "OutsideWindow"
                | "InNoWindow"
                | "Asking"
                | "BadInput"
                | "CannotLoad"
                | "LocalOnly"
                | "Failed"
                | "Unknown"
        )
    }

    fn unit(tag: &str) -> Option<Self> {
        match tag {
            "NoDevRunNow" => Some(RefusalKind::NoDevRunNow),
            "NeverDevRun" => Some(RefusalKind::NeverDevRun),
            "BadInput" => Some(RefusalKind::BadInput),
            "CannotLoad" => Some(RefusalKind::CannotLoad),
            "LocalOnly" => Some(RefusalKind::LocalOnly),
            "Failed" => Some(RefusalKind::Failed),
            "Unknown" => Some(RefusalKind::Unknown),
            _ => None,
        }
    }

    fn payload<'de, D: Deserializer<'de>>(tag: &str, deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct WindowDetails {
            window: WindowId,
        }
        #[derive(Deserialize)]
        struct OutsideWindowDetails {
            width: u32,
            height: u32,
            x: f64,
            y: f64,
            scale: f64,
        }
        match tag {
            "NoWindow" => {
                let WindowDetails { window } = Deserialize::deserialize(deserializer)?;
                Ok(RefusalKind::NoWindow { window })
            }
            "OutsideWindow" => {
                let OutsideWindowDetails {
                    width,
                    height,
                    x,
                    y,
                    scale,
                } = Deserialize::deserialize(deserializer)?;
                Ok(RefusalKind::OutsideWindow {
                    width,
                    height,
                    x,
                    y,
                    scale,
                })
            }
            "InNoWindow" => {
                let WindowDetails { window } = Deserialize::deserialize(deserializer)?;
                Ok(RefusalKind::InNoWindow { window })
            }
            "Asking" => Deserialize::deserialize(deserializer).map(RefusalKind::Asking),
            _ => unreachable!(),
        }
    }
}

impl<'de> Deserialize<'de> for RefusalKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        adjacent::deserialize(deserializer)
    }
}
