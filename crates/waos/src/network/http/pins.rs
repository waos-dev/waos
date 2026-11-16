//! The hosts a program's requests are pinned to: a host the program's lookup answered already, to
//! pick a request's way past a proxy (`route`), answers that lookup's addresses while the request
//! is sent, so the connection made for it goes where the lookup was checked, and the host is not
//! looked up a second time.
//!
//! The invariant: a host is answered without a lookup only while a request holds the answer the
//! program's lookup gave for it.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use super::route::Answered;
use crate::sync::locked;

/// The hosts pinned, each to the addresses it answers.
#[derive(Default)]
pub(super) struct Pins(Mutex<HashMap<String, Pin>>);

/// What a host is pinned to: the addresses the program's lookup answered last, and how many
/// requests being sent hold the pin.
struct Pin {
    addresses: Vec<IpAddr>,
    holders: usize,
}

/// A request's hold on a host's pin, until this is dropped: the last hold dropped unpins the
/// host.
pub(super) struct Pinned {
    pins: Arc<Pins>,
    host: String,
}

impl Pins {
    /// Answers `answered`'s host with its addresses until every pin of it held is dropped: a
    /// newer answer replaces the addresses of a host pinned already.
    pub(super) fn pin(self: &Arc<Self>, answered: Answered) -> Pinned {
        let Answered { host, addresses } = answered;
        let mut pinned = locked(&self.0);
        let pin = pinned.entry(host.clone()).or_insert(Pin {
            addresses: Vec::new(),
            holders: 0,
        });
        pin.addresses = addresses;
        pin.holders += 1;
        Pinned {
            pins: Arc::clone(self),
            host,
        }
    }

    /// The addresses `host` is pinned to, if it is.
    pub(super) fn addresses_of(&self, host: &str) -> Option<Vec<IpAddr>> {
        locked(&self.0).get(host).map(|pin| pin.addresses.clone())
    }
}

impl Drop for Pinned {
    fn drop(&mut self) {
        let mut pinned = locked(&self.pins.0);
        let Some(pin) = pinned.get_mut(&self.host) else {
            return;
        };
        pin.holders -= 1;
        if pin.holders == 0 {
            pinned.remove(&self.host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "pinned.test";

    fn address(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn pinned(pins: &Arc<Pins>, to: &str) -> Pinned {
        pins.pin(Answered {
            host: HOST.to_owned(),
            addresses: vec![address(to)],
        })
    }

    #[test]
    fn a_host_stays_pinned_until_the_last_request_holding_it_ends() {
        let pins = Arc::new(Pins::default());
        let first = pinned(&pins, "127.0.0.1");
        let second = pinned(&pins, "127.0.0.2");
        assert_eq!(
            pins.addresses_of(HOST),
            Some(vec![address("127.0.0.2")]),
            "the newer answer"
        );
        drop(first);
        assert_eq!(
            pins.addresses_of(HOST),
            Some(vec![address("127.0.0.2")]),
            "still held"
        );
        drop(second);
        assert_eq!(pins.addresses_of(HOST), None, "looked up again");
    }
}
