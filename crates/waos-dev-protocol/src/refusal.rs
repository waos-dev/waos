//! Why a request was not carried out: a kind an agent can act on, and a message for a person.
//!
//! The invariant: CBOR names each variant and field, so a kind added leaves the ones before it as
//! they were, and a `waos` older than the WaOS it reaches reads a kind it does not know as
//! `Unknown`, never breaking the connection over a refusal it can still show.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::adjacent::adjacent_unknown;
use crate::messages::WindowId;
use crate::settings_rows::Asked;

/// Why a request was not carried out, and the words for its user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Refusal {
    #[serde(flatten)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", tag = "kind", content = "data")]
pub enum RefusalKind {
    /// The program does not run now.
    NotRunning,
    /// No program of the id is installed.
    NotInstalled,
    /// The program has no window `window`.
    NoWindow { window: WindowId },
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
    InNoWindow { window: WindowId },
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
    /// The request or an option inside it came from a newer client that this WaOS does not know.
    NotServed,
    /// The request itself could not be carried out, as a store that cannot be read or a picture
    /// that cannot be taken: the message says what failed.
    Failed,
    #[serde(skip)]
    Unknown,
}

adjacent_unknown!(RefusalKind);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_encodes_kind_and_data_once_without_nesting() {
        let refusal = Refusal::new(
            RefusalKind::NoWindow {
                window: WindowId(2),
            },
            "no window 2",
        );
        let mut bytes = Vec::new();
        ciborium::into_writer(&refusal, &mut bytes).unwrap();
        let value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
        let ciborium::Value::Map(entries) = &value else {
            panic!("a map: {value:?}");
        };
        let keys: Vec<&str> = entries.iter().filter_map(|(k, _)| k.as_text()).collect();
        assert_eq!(keys, ["kind", "data", "message"]);
        let back: Refusal = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(back, refusal);
    }
}
