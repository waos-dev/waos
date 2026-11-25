//! How a WaOS with developer access announces itself on the local network, so `waos devices` finds
//! it: a DNS-SD service of type [`SERVICE`] on the port it listens on, its instance named as the
//! device names itself, its TXT record [`Announced`]. Finding is all it is for: a computer still
//! pairs, and trusts the device by the fingerprint pairing proved, never by what an announcement
//! says.

use std::str::FromStr;

use crate::hello::MAJOR;
use crate::network::Fingerprint;

/// The service type a WaOS with developer access announces.
pub const SERVICE: &str = "_waos-dev._tcp";

const PROTOCOL: &str = "protocol";
const FINGERPRINT: &str = "fingerprint";
const SYSTEM: &str = "system";

/// What an announcement's TXT record says: the protocol's major the WaOS speaks, the fingerprint
/// of its certificate, by which a computer knows a device it is paired with wherever it is now,
/// and its system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announced {
    pub protocol: u32,
    pub fingerprint: Fingerprint,
    pub system: String,
}

impl Announced {
    /// What this build announces for the certificate `fingerprint` on `system`.
    pub fn new(fingerprint: Fingerprint, system: &str) -> Announced {
        Announced {
            protocol: MAJOR,
            fingerprint,
            system: system.to_owned(),
        }
    }

    /// The TXT record's entries, each a key and its value.
    pub fn entries(&self) -> Vec<(&'static str, String)> {
        vec![
            (PROTOCOL, self.protocol.to_string()),
            (FINGERPRINT, self.fingerprint.to_string()),
            (SYSTEM, self.system.clone()),
        ]
    }

    /// What a TXT record says, `value` answering each key's value; none where it names no
    /// fingerprint or protocol this build reads, as a record of something else.
    pub fn read<'a>(value: impl Fn(&str) -> Option<&'a str>) -> Option<Announced> {
        Some(Announced {
            protocol: value(PROTOCOL)?.parse().ok()?,
            fingerprint: Fingerprint::from_str(value(FINGERPRINT)?).ok()?,
            system: value(SYSTEM).unwrap_or_default().to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_announcement_reads_back_from_its_entries() {
        let announced = Announced::new(Fingerprint([7; 32]), "macos");
        let entries = announced.entries();
        let value = |key: &str| {
            entries
                .iter()
                .find(|(named, _)| *named == key)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(Announced::read(value), Some(announced));
        assert_eq!(Announced::read(|_| None), None);
    }
}
