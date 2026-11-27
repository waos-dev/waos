//! The addresses a program's lookups answered, each with the names it was looked up by: what the
//! connect check reads to judge an address by the domain rule, which names domains, not addresses.
//!
//! An entry lasts [`LIFETIME`] after the last lookup that answered it. A lookup gives no time to
//! live, so the record keeps an address longer than most DNS answers last, long enough that a
//! program connecting soon after its lookup, or reconnecting a while later, is judged by the
//! name it looked up; a program that keeps an address longer without looking it up again has it
//! judged as a bare address.
//!
//! The invariant: the record holds names, never verdicts. Whether a name is allowed is read from
//! the grants at each check, so a rule the user changes applies to addresses looked up before.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::sync::locked;

/// How long an address stays recorded after the last lookup that answered it.
pub(crate) const LIFETIME: Duration = Duration::from_secs(60 * 60);

/// The addresses one program's lookups answered.
#[derive(Default)]
pub(crate) struct AddressRecord {
    /// Each address, with each name it was looked up by and when that entry expires.
    entries: Mutex<HashMap<IpAddr, HashMap<String, Instant>>>,
}

impl AddressRecord {
    /// Keeps that a lookup of `name` answered `addresses`, now.
    pub(crate) fn keep(&self, name: &str, addresses: &[IpAddr]) {
        self.keep_at(name, addresses, Instant::now());
    }

    /// The names `address` was looked up by, of entries still kept.
    pub(crate) fn names_of(&self, address: IpAddr) -> Vec<String> {
        self.names_of_at(address, Instant::now())
    }

    fn keep_at(&self, name: &str, addresses: &[IpAddr], now: Instant) {
        let mut entries = locked(&self.entries);
        entries.retain(|_, names| {
            names.retain(|_, expires| *expires > now);
            !names.is_empty()
        });
        for address in addresses {
            let names = entries.entry(address.to_canonical()).or_default();
            names.insert(name.to_owned(), now + LIFETIME);
        }
    }

    fn names_of_at(&self, address: IpAddr, now: Instant) -> Vec<String> {
        let entries = locked(&self.entries);
        let Some(names) = entries.get(&address.to_canonical()) else {
            return Vec::new();
        };
        names
            .iter()
            .filter(|(_, expires)| **expires > now)
            .map(|(name, _)| name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_kept_with_every_name_that_answered_it_for_its_lifetime() {
        let record = AddressRecord::default();
        let start = Instant::now();
        let shared: IpAddr = "93.184.216.34".parse().unwrap();
        record.keep_at("example.com", &[shared], start);
        record.keep_at("cdn.example.org", &[shared], start + LIFETIME / 2);
        let mut names = record.names_of_at(shared, start + LIFETIME / 4);
        names.sort();
        assert_eq!(names, ["cdn.example.org", "example.com"]);
        assert_eq!(
            record.names_of_at(shared, start + LIFETIME + Duration::from_secs(1)),
            ["cdn.example.org"],
            "the first entry expired"
        );
        assert!(
            record
                .names_of_at("8.8.8.8".parse().unwrap(), start)
                .is_empty()
        );
    }

    #[test]
    fn an_address_written_as_ipv6_is_the_ipv4_address_it_is() {
        let record = AddressRecord::default();
        record.keep("example.com", &["93.184.216.34".parse().unwrap()]);
        assert_eq!(
            record.names_of("::ffff:93.184.216.34".parse().unwrap()),
            ["example.com"]
        );
    }
}
